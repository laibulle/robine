use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use actix_files::Files;
use actix_web::{App, HttpServer, web};
use anyhow::Context;
use chrono::Utc;
use robine_api_contract::{HueBridgeCandidate, HuePairRequest, HuePairResponse};
use robine_api_http::{
    HueAdministration, HueAdministrationError, ServerState, configure as configure_api,
};
use robine_application::{ApplicationError, CommandDispatcher, FlowService, HomeService};
use robine_domain::Command;
use robine_integration_hue::{
    HueAdapter, HueBridgeClient, HueCommandDispatcher, HueError, HueHttpBridgeClient,
    discover_bridges,
};
use robine_mcp_http::{
    AuthenticationError, McpAuthenticator, McpHttpState, configure as configure_mcp,
};
use robine_mcp_tools::McpTools;
use robine_mcp_types::{Scope, Scopes};
use robine_secret_store::{MacOsKeychainSecretStore, SecretStore};
use robine_store_sqlite::{HueBridgeConfiguration, SqliteStore};
use sha2::{Digest, Sha256};

/// Point de routage mutable : les use-cases restent indépendants des
/// intégrations, tandis que les adaptateurs ajoutés par l'administrateur
/// peuvent recevoir les commandes sans redémarrer le serveur.
#[derive(Default)]
struct SwitchingCommandDispatcher(RwLock<Option<Arc<dyn CommandDispatcher>>>);
impl SwitchingCommandDispatcher {
    fn replace(
        &self,
        dispatcher: Arc<dyn CommandDispatcher>,
    ) -> Result<(), HueAdministrationError> {
        *self.0.write().map_err(|_| {
            HueAdministrationError::Unavailable("command router unavailable".into())
        })? = Some(dispatcher);
        Ok(())
    }
}
impl CommandDispatcher for SwitchingCommandDispatcher {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let dispatcher = self
            .0
            .read()
            .map_err(|_| ApplicationError::Infrastructure("command router unavailable".into()))?
            .clone();
        dispatcher
            .ok_or_else(|| {
                ApplicationError::Infrastructure("no adapter can dispatch this command".into())
            })?
            .dispatch(command)
    }
}

struct HueRuntimeAdministration {
    service: HomeService,
    store: Arc<SqliteStore>,
    dispatcher: Arc<SwitchingCommandDispatcher>,
    secrets: Arc<dyn SecretStore>,
    active: Mutex<Option<Arc<HueAdapter<HueHttpBridgeClient>>>>,
}
impl HueRuntimeAdministration {
    fn new(
        service: HomeService,
        store: Arc<SqliteStore>,
        dispatcher: Arc<SwitchingCommandDispatcher>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            service,
            store,
            dispatcher,
            secrets,
            active: Mutex::new(None),
        }
    }
}
impl HueAdministration for HueRuntimeAdministration {
    fn discover(&self) -> Result<Vec<HueBridgeCandidate>, HueAdministrationError> {
        discover_bridges(std::time::Duration::from_secs(2))
            .map(|bridges| {
                bridges
                    .into_iter()
                    .map(|bridge| HueBridgeCandidate {
                        name: bridge.name,
                        host: bridge.host,
                        addresses: bridge.addresses,
                    })
                    .collect()
            })
            .map_err(|_| HueAdministrationError::Unavailable("local Hue discovery failed".into()))
    }

    fn pair(&self, request: HuePairRequest) -> Result<HuePairResponse, HueAdministrationError> {
        let authority = request.authority.trim();
        if authority.is_empty()
            || authority.contains('/')
            || authority.contains('@')
            || request.certificate_pem.is_empty()
        {
            return Err(HueAdministrationError::InvalidRequest(
                "invalid bridge configuration".into(),
            ));
        }
        let fingerprint = format!("{:x}", Sha256::digest(request.certificate_pem.as_bytes()));
        if !constant_time_eq(
            fingerprint.as_bytes(),
            request
                .certificate_sha256
                .trim()
                .to_ascii_lowercase()
                .as_bytes(),
        ) {
            return Err(HueAdministrationError::InvalidRequest(
                "certificate fingerprint mismatch".into(),
            ));
        }
        let anonymous = HueHttpBridgeClient::with_pinned_certificate(
            authority,
            request.certificate_pem.as_bytes(),
            None,
        )
        .map_err(hue_administration_error)?;
        let application_key = anonymous
            .create_application_key()
            .map_err(hue_administration_error)?;
        let client = Arc::new(
            HueHttpBridgeClient::with_pinned_certificate(
                authority,
                request.certificate_pem.as_bytes(),
                Some(application_key.clone()),
            )
            .map_err(hue_administration_error)?,
        );
        let hue_dispatcher = Arc::new(HueCommandDispatcher::new(client.clone()));
        let adapter = Arc::new(HueAdapter::with_dispatcher(
            client,
            self.service.clone(),
            hue_dispatcher.clone(),
        ));
        let devices = adapter.synchronize(Utc::now()).map_err(|_| {
            HueAdministrationError::Unavailable("initial synchronization failed".into())
        })?;
        // La clé est écrite seulement après validation TLS et synchronisation.
        let secret_name = format!("hue:{}", authority);
        self.secrets
            .put(&secret_name, &application_key)
            .map_err(|_| {
                HueAdministrationError::Unavailable(
                    "could not securely store Hue credentials".into(),
                )
            })?;
        self.store
            .save_hue_bridge(&HueBridgeConfiguration {
                authority: authority.into(),
                certificate_pem: request.certificate_pem,
                certificate_sha256: fingerprint,
                secret_name,
            })
            .map_err(|_| {
                HueAdministrationError::Unavailable(
                    "could not persist Hue bridge configuration".into(),
                )
            })?;
        self.dispatcher.replace(hue_dispatcher)?;
        *self
            .active
            .lock()
            .map_err(|_| HueAdministrationError::Unavailable("Hue state unavailable".into()))? =
            Some(adapter.clone());
        spawn_hue_event_listener(adapter, self.service.clone());
        let adapter_id = devices
            .first()
            .map(|device| device.adapter_id.0.clone())
            .unwrap_or_else(|| format!("hue:{authority}"));
        Ok(HuePairResponse {
            adapter_id,
            discovered_devices: devices.len(),
        })
    }
    fn synchronize(&self) -> Result<usize, HueAdministrationError> {
        let adapter = self
            .active
            .lock()
            .map_err(|_| HueAdministrationError::Unavailable("Hue state unavailable".into()))?
            .clone()
            .ok_or_else(|| {
                HueAdministrationError::InvalidRequest(
                    "no Hue bridge is paired in this server session".into(),
                )
            })?;
        adapter
            .synchronize(Utc::now())
            .map(|devices| devices.len())
            .map_err(|_| HueAdministrationError::Unavailable("synchronization failed".into()))
    }
}

impl HueRuntimeAdministration {
    /// La restauration est volontairement tolérante : une absence de bridge ou
    /// de clé ne doit jamais empêcher le serveur HTTP de démarrer.
    fn restore_paired_bridges(&self) {
        let configurations = match self.store.list_hue_bridges() {
            Ok(configurations) => configurations,
            Err(_) => return,
        };
        for configuration in configurations {
            let Ok(Some(application_key)) = self.secrets.get(&configuration.secret_name) else {
                continue;
            };
            let Ok(client) = HueHttpBridgeClient::with_pinned_certificate(
                &configuration.authority,
                configuration.certificate_pem.as_bytes(),
                Some(application_key),
            ) else {
                continue;
            };
            let client = Arc::new(client);
            let hue_dispatcher = Arc::new(HueCommandDispatcher::new(client.clone()));
            let adapter = Arc::new(HueAdapter::with_dispatcher(
                client,
                self.service.clone(),
                hue_dispatcher.clone(),
            ));
            if adapter.synchronize(Utc::now()).is_ok() {
                let _ = self.dispatcher.replace(hue_dispatcher);
                if let Ok(mut active) = self.active.lock() {
                    *active = Some(adapter.clone());
                }
                spawn_hue_event_listener(adapter, self.service.clone());
            }
        }
    }
}

fn hue_administration_error(error: HueError) -> HueAdministrationError {
    match error {
        HueError::LinkButtonNotPressed => HueAdministrationError::ButtonNotPressed,
        HueError::InvalidResponse(_) => {
            HueAdministrationError::InvalidRequest("invalid Hue response".into())
        }
        _ => HueAdministrationError::Unavailable("Hue bridge unavailable".into()),
    }
}
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}
fn spawn_hue_event_listener(adapter: Arc<HueAdapter<HueHttpBridgeClient>>, service: HomeService) {
    std::thread::Builder::new()
        .name("robine-hue-events".into())
        .spawn(move || {
            loop {
                if adapter.listen_for_events().is_err() {
                    let _ = service.update_adapter_health(robine_domain::AdapterHealth {
                        adapter_id: robine_domain::AdapterId::new("hue:event-stream")
                            .expect("static adapter id"),
                        status: robine_domain::AdapterStatus::Degraded,
                        detail: Some("Hue event stream disconnected; retrying".into()),
                        observed_at: Utc::now(),
                    });
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let _ = adapter.synchronize(Utc::now());
                }
            }
        })
        .expect("spawn Hue event listener");
}

/// Consomme les changements déjà publiés par le store et exécute les Flows
/// correspondants. Les commandes générées repassent par `HomeService`, donc un
/// Flow ne peut pas contourner les adaptateurs ni leurs contrôles de capacité.
fn spawn_automation_engine(store: Arc<SqliteStore>, service: HomeService) {
    let flows = FlowService::new(store.clone(), store.clone());
    actix_web::rt::spawn(async move {
        let mut events = store.subscribe_events();
        let mut scheduler = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = scheduler.tick() => {
                    let flows = flows.clone();
                    let service = service.clone();
                    let _ = actix_web::rt::task::spawn_blocking(move || {
                        for execution in flows.resume_due(&service, Utc::now()) {
                            if let Err(error) = execution { tracing::warn!(error = %error, "Flow resume failed"); }
                        }
                    }).await;
                }
                event = events.recv() => match event {
                    Ok(event) => {
                        if let robine_domain::EventData::StateReported { state } = event.data {
                            let flows = flows.clone();
                            let service = service.clone();
                            let _ = actix_web::rt::task::spawn_blocking(move || {
                                for execution in flows.execute_state_triggered(&state, &service, Utc::now()) {
                                    if let Err(error) = execution { tracing::warn!(error = %error, "Flow execution failed"); }
                                }
                            }).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => tracing::warn!(count, "automation engine is resynchronizing after event lag"),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    });
}

struct RuntimeMcpAuthenticator {
    store: Arc<SqliteStore>,
}

impl McpAuthenticator for RuntimeMcpAuthenticator {
    fn authenticate(&self, bearer: &str) -> Result<Scopes, AuthenticationError> {
        self.store
            .authenticate_mcp_read(bearer, Utc::now())
            .ok()
            .filter(|valid| *valid)
            .map(|_| Scopes::new([Scope::Read]))
            .ok_or(AuthenticationError::Invalid)
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let data_dir = env::var_os("ROBINE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));
    std::fs::create_dir_all(&data_dir).context("creating Robine data directory")?;
    let store = Arc::new(SqliteStore::open(data_dir.join("robine.sqlite3"))?);
    let dispatcher = Arc::new(SwitchingCommandDispatcher::default());
    let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
    let bind = env::var("ROBINE_BIND").unwrap_or_else(|_| "127.0.0.1:3030".into());
    let web_dir = env::var_os("ROBINE_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("apps/web/dist"));
    let secrets: Arc<dyn SecretStore> =
        Arc::new(MacOsKeychainSecretStore::new("io.robine.server")?);
    let hue = Arc::new(HueRuntimeAdministration::new(
        service.clone(),
        store.clone(),
        dispatcher,
        secrets,
    ));
    hue.restore_paired_bridges();
    spawn_automation_engine(store.clone(), service.clone());
    let api_state = ServerState::new(service.clone(), store.clone()).with_hue(hue);
    let mcp_state = McpHttpState::new(
        McpTools::new(service),
        Arc::new(RuntimeMcpAuthenticator { store }),
        std::iter::empty(),
    );
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(api_state.clone()))
            .configure(configure_api)
            .configure(|configuration| configure_mcp(configuration, mcp_state.clone()))
            .service(Files::new("/", web_dir.clone()).index_file("index.html"))
    })
    .bind(&bind)?
    .run()
    .await?;
    Ok(())
}
