use std::{
    collections::HashSet,
    env,
    fs::{File, OpenOptions},
    io::BufReader,
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command as ProcessCommand},
    sync::{Arc, Mutex, RwLock},
};

use actix_files::Files;
use actix_web::{App, HttpServer, http::header, middleware::DefaultHeaders, web};
use anyhow::Context;
use chrono::Utc;
use fs2::FileExt;
use rand::random;
use robine_api_contract::{HueBridgeCandidate, HuePairRequest, HuePairResponse, HueRoomSuggestion};
use robine_api_http::{
    BackupAdministration, HueAdministration, HueAdministrationError, MatterAdministration,
    MatterAdministrationError, ServerState, configure as configure_api,
};
use robine_application::{
    ApplicationError, CommandDispatcher, FlowService, HomeRepository, HomeService,
};
use robine_domain::Command;
use robine_integration_hue::{
    HueAdapter, HueBridgeClient, HueCommandDispatcher, HueError, HueHttpBridgeClient,
    HueReconnectBackoff, discover_bridges,
};
use robine_integration_matter::{
    ADAPTER_ID as MATTER_ADAPTER_ID, LocalMatterClient, MatterAdapter, MatterClient,
    MatterCommandDispatcher,
};
use robine_mcp_http::{
    ApprovalError, AuthenticationError, McpApprovalAuthorizer, McpAuthenticator, McpHttpState,
    configure as configure_mcp,
};
use robine_mcp_tools::McpTools;
use robine_mcp_types::{McpPrincipal, Scope, Scopes};
use robine_protocol_mqtt::{
    ADAPTER_ID as MQTT_ADAPTER_ID, MqttAdapter, MqttBrokerConfiguration, MqttTlsConfiguration,
    RumqttPublisher,
};
use robine_secret_store::{MacOsKeychainSecretStore, SecretStore};
use robine_store_sqlite::{HueBridgeConfiguration, SqliteStore};
use sha2::{Digest, Sha256};

const RUNTIME_LOCK_FILE: &str = ".robine-runtime.lock";

/// Verrou advisory détenu pendant toute la durée du serveur. Il est libéré par
/// l'OS si le processus se termine brutalement. La restauration hors ligne
/// acquiert le même verrou et refuse donc de s'exécuter lorsque Robine tourne.
struct RuntimeDataLock {
    _file: File,
}

impl RuntimeDataLock {
    fn acquire(data_dir: &std::path::Path) -> anyhow::Result<Self> {
        let path = data_dir.join(RUNTIME_LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("opening runtime lock {}", path.display()))?;
        file.try_lock_exclusive().with_context(|| {
            format!(
                "Robine is already active for {}; stop it before maintenance",
                data_dir.display()
            )
        })?;
        Ok(Self { _file: file })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RuntimeMode {
    Serve,
    Restore { manifest: PathBuf },
}

fn runtime_mode(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> anyhow::Result<RuntimeMode> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Ok(RuntimeMode::Serve);
    };
    if command != "restore" {
        anyhow::bail!(
            "usage: robine-runtime [restore --manifest <backup.manifest.json> --confirm]"
        );
    }
    let mut manifest = None;
    let mut confirmed = false;
    while let Some(argument) = arguments.next() {
        if argument == "--manifest" {
            let Some(value) = arguments.next() else {
                anyhow::bail!("restore requires a manifest after --manifest");
            };
            if manifest.replace(PathBuf::from(value)).is_some() {
                anyhow::bail!("restore accepts exactly one --manifest");
            }
        } else if argument == "--confirm" {
            confirmed = true;
        } else {
            anyhow::bail!("unknown restore argument {:?}", argument);
        }
    }
    let manifest = manifest.context("restore requires --manifest <backup.manifest.json>")?;
    if !confirmed {
        anyhow::bail!(
            "restore changes the active database; repeat with --confirm after stopping Robine"
        );
    }
    Ok(RuntimeMode::Restore { manifest })
}

fn restore_from_manifest(data_dir: &std::path::Path, manifest: PathBuf) -> anyhow::Result<()> {
    let backups = data_dir.join("backups");
    let backups = std::fs::canonicalize(&backups)
        .with_context(|| format!("opening backup directory {}", backups.display()))?;
    let manifest = std::fs::canonicalize(&manifest)
        .with_context(|| format!("opening backup manifest {}", manifest.display()))?;
    if manifest.parent() != Some(backups.as_path()) {
        anyhow::bail!(
            "backup manifest must be a direct child of {}",
            backups.display()
        );
    }
    let _lock = RuntimeDataLock::acquire(data_dir)?;
    let manifest: robine_store_backup::BackupManifest =
        serde_json::from_slice(&std::fs::read(&manifest).context("reading backup manifest")?)
            .context("parsing backup manifest")?;
    let previous = robine_store_backup::restore_snapshot(
        &data_dir.join("robine.sqlite3"),
        &backups,
        &manifest,
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    println!(
        "Restored {}. Previous database retained at {}",
        manifest.database_file,
        previous.display()
    );
    Ok(())
}

/// Une écoute hors loopback expose des commandes domestiques : elle doit donc
/// toujours passer par TLS. Les chemins restent une configuration de runtime,
/// sans jamais être écrits dans SQLite ni exposés par l'API.
fn tls_for_listener(
    bind: &str,
    certificate: Option<PathBuf>,
    private_key: Option<PathBuf>,
) -> anyhow::Result<Option<rustls::ServerConfig>> {
    match (certificate, private_key) {
        (None, None) if is_loopback_listener(bind) => Ok(None),
        (None, None) => anyhow::bail!(
            "ROBINE_TLS_CERT and ROBINE_TLS_KEY are required when ROBINE_BIND is not loopback"
        ),
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!("ROBINE_TLS_CERT and ROBINE_TLS_KEY must be configured together")
        }
        (Some(certificate), Some(private_key)) => {
            let mut certificate_reader =
                BufReader::new(File::open(&certificate).with_context(|| {
                    format!("opening TLS certificate {}", certificate.display())
                })?);
            let certificates = rustls_pemfile::certs(&mut certificate_reader)
                .collect::<Result<Vec<_>, _>>()
                .context("reading TLS certificate PEM")?;
            if certificates.is_empty() {
                anyhow::bail!("ROBINE_TLS_CERT contains no certificate")
            }
            let mut key_reader =
                BufReader::new(File::open(&private_key).with_context(|| {
                    format!("opening TLS private key {}", private_key.display())
                })?);
            let private_key = rustls_pemfile::private_key(&mut key_reader)
                .context("reading TLS private key PEM")?
                .context("ROBINE_TLS_KEY contains no private key")?;
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certificates, private_key)
                .context("building TLS server configuration")
                .map(Some)
        }
    }
}

fn is_loopback_listener(bind: &str) -> bool {
    bind.parse::<SocketAddr>()
        .map(|address| address.ip().is_loopback())
        .unwrap_or_else(|_| bind.starts_with("localhost:"))
}

/// Point de routage mutable. Chaque adaptateur tente seulement les entités
/// qu'il connaît ; le premier à accepter une commande la prend en charge.
#[derive(Default)]
struct SwitchingCommandDispatcher(RwLock<Vec<Arc<dyn CommandDispatcher>>>);
impl SwitchingCommandDispatcher {
    fn register(
        &self,
        dispatcher: Arc<dyn CommandDispatcher>,
    ) -> Result<(), HueAdministrationError> {
        self.0
            .write()
            .map_err(|_| HueAdministrationError::Unavailable("command router unavailable".into()))?
            .push(dispatcher);
        Ok(())
    }
}
impl CommandDispatcher for SwitchingCommandDispatcher {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let dispatchers = self
            .0
            .read()
            .map_err(|_| ApplicationError::Infrastructure("command router unavailable".into()))?
            .clone();
        let mut last_error = None;
        for dispatcher in dispatchers {
            match dispatcher.dispatch(command.clone()) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            ApplicationError::Infrastructure("no adapter can dispatch this command".into())
        }))
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
            || authority.chars().any(|character| {
                character.is_whitespace() || matches!(character, '/' | '@' | '?' | '#')
            })
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
        self.dispatcher.register(hue_dispatcher)?;
        *self
            .active
            .lock()
            .map_err(|_| HueAdministrationError::Unavailable("Hue state unavailable".into()))? =
            Some(adapter.clone());
        spawn_hue_event_listener(adapter);
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
    fn room_suggestions(&self) -> Result<Vec<HueRoomSuggestion>, HueAdministrationError> {
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
            .room_suggestions()
            .map(|suggestions| {
                suggestions
                    .into_iter()
                    .map(|suggestion| HueRoomSuggestion {
                        name: suggestion.name,
                        entity_ids: suggestion.entity_ids,
                    })
                    .collect()
            })
            .map_err(|_| {
                HueAdministrationError::Unavailable("Hue room suggestions are unavailable".into())
            })
    }
    fn import_room(
        &self,
        suggestion: HueRoomSuggestion,
    ) -> Result<robine_domain::Area, HueAdministrationError> {
        let available = self.room_suggestions()?;
        if suggestion.name.trim().is_empty()
            || suggestion.entity_ids.is_empty()
            || !available.contains(&suggestion)
        {
            return Err(HueAdministrationError::InvalidRequest(
                "the Hue room suggestion is no longer current".into(),
            ));
        }
        if let Some(area) = self.existing_imported_room(&suggestion).map_err(|_| {
            HueAdministrationError::Unavailable("could not inspect Robine rooms".into())
        })? {
            return Ok(area);
        }
        self.service
            .create_area_with_entities(suggestion.name, suggestion.entity_ids, Utc::now())
            .map_err(|_| {
                HueAdministrationError::Unavailable("could not import the Hue room".into())
            })
    }
}

impl HueRuntimeAdministration {
    /// Une répétition réseau du même import est sans effet. La comparaison ne
    /// porte que sur les IDs canoniques Robine, jamais sur une ressource Hue.
    fn existing_imported_room(
        &self,
        suggestion: &HueRoomSuggestion,
    ) -> Result<Option<robine_domain::Area>, ApplicationError> {
        let requested = suggestion
            .entity_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let devices = self.service.list_devices()?;
        for area in self.service.list_areas()? {
            if area.name != suggestion.name {
                continue;
            }
            let assigned = devices
                .iter()
                .flat_map(|device| device.entities.iter())
                .filter(|entity| entity.area_id.as_ref() == Some(&area.id))
                .map(|entity| entity.id.clone())
                .collect::<HashSet<_>>();
            if assigned == requested {
                return Ok(Some(area));
            }
        }
        Ok(None)
    }

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
                let _ = self.dispatcher.register(hue_dispatcher);
                if let Ok(mut active) = self.active.lock() {
                    *active = Some(adapter.clone());
                }
                spawn_hue_event_listener(adapter);
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
fn spawn_hue_event_listener(adapter: Arc<HueAdapter<HueHttpBridgeClient>>) {
    std::thread::Builder::new()
        .name("robine-hue-events".into())
        .spawn(move || {
            let policy = HueReconnectBackoff::default();
            let mut consecutive_failures = 0u8;
            loop {
                let connected_at = std::time::Instant::now();
                if let Err(error) = adapter.listen_for_events() {
                    // Une connexion réellement stable mérite une nouvelle
                    // fenêtre de reprise courte ; une série de coupures rapides
                    // continue au contraire son backoff jusqu'au plafond.
                    if connected_at.elapsed() >= std::time::Duration::from_secs(30) {
                        consecutive_failures = 0;
                    }
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    tracing::warn!(error = %error, consecutive_failures, "Hue event stream disconnected");
                    let _ = adapter.mark_event_stream_disconnected(Utc::now());
                    loop {
                        let delay = policy.delay(consecutive_failures, random());
                        tracing::info!(
                            consecutive_failures,
                            delay_milliseconds = delay.as_millis(),
                            "Hue reconnect is waiting before full synchronization"
                        );
                        std::thread::sleep(delay);
                        match adapter.synchronize(Utc::now()) {
                            Ok(_) => break,
                            Err(error) => {
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                tracing::warn!(error = %error, consecutive_failures, "Hue resynchronization failed");
                                let _ = adapter.mark_event_stream_disconnected(Utc::now());
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn Hue event listener");
}

fn start_configured_mqtt(
    service: HomeService,
    dispatcher: Arc<SwitchingCommandDispatcher>,
    secrets: Arc<dyn SecretStore>,
) {
    let Ok(host) = env::var("ROBINE_MQTT_HOST") else {
        return;
    };
    let tls_requested = env::var("ROBINE_MQTT_TLS")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"));
    let tls = if tls_requested {
        let ca_certificate_pem = secrets
            .get("mqtt:ca-certificate")
            .ok()
            .flatten()
            .map(String::into_bytes);
        let client_certificate = secrets
            .get("mqtt:client-certificate")
            .ok()
            .flatten()
            .map(String::into_bytes);
        let client_private_key = secrets
            .get("mqtt:client-private-key")
            .ok()
            .flatten()
            .map(String::into_bytes);
        let client_auth = match (client_certificate, client_private_key) {
            (None, None) => None,
            (Some(certificate), Some(private_key)) => Some((certificate, private_key)),
            _ => {
                let _ = service.update_adapter_health(robine_domain::AdapterHealth {
                    adapter_id: robine_domain::AdapterId::new(MQTT_ADAPTER_ID)
                        .expect("static adapter id"),
                    status: robine_domain::AdapterStatus::Degraded,
                    detail: Some("MQTT mTLS requires both certificate and private key".into()),
                    observed_at: Utc::now(),
                });
                return;
            }
        };
        Some(match ca_certificate_pem {
            Some(ca_certificate_pem) => MqttTlsConfiguration::Custom {
                ca_certificate_pem,
                client_auth,
            },
            None => MqttTlsConfiguration::SystemRoots,
        })
    } else {
        None
    };
    let port = env::var("ROBINE_MQTT_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(if tls_requested { 8883 } else { 1883 });
    let username = env::var("ROBINE_MQTT_USERNAME").ok();
    let password = secrets.get("mqtt:local").ok().flatten();
    let configuration = MqttBrokerConfiguration {
        host,
        port,
        client_id: format!("robine-{}", uuid::Uuid::new_v4()),
        username,
        tls,
    };
    let Ok((publisher, receiver)) = RumqttPublisher::connect(configuration, password) else {
        let _ = service.update_adapter_health(robine_domain::AdapterHealth {
            adapter_id: robine_domain::AdapterId::new(MQTT_ADAPTER_ID).expect("static adapter id"),
            status: robine_domain::AdapterStatus::Degraded,
            detail: Some("MQTT broker connection could not start".into()),
            observed_at: Utc::now(),
        });
        return;
    };
    let adapter = Arc::new(MqttAdapter::new(publisher, service.clone()));
    if dispatcher.register(adapter.clone()).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("robine-mqtt-ingest".into())
        .spawn(move || {
            while let Ok(message) = receiver.recv() {
                let result = if message.topic.starts_with("robine/v1/discovery/")
                    || message.topic.starts_with("homeassistant/")
                {
                    adapter
                        .ingest_discovery(&message.topic, &message.payload, Utc::now())
                        .map(|_| ())
                } else {
                    adapter.ingest_state(&message.topic, &message.payload, Utc::now())
                };
                if result.is_err() {
                    let _ = service.update_adapter_health(robine_domain::AdapterHealth {
                        adapter_id: robine_domain::AdapterId::new(MQTT_ADAPTER_ID)
                            .expect("static adapter id"),
                        status: robine_domain::AdapterStatus::Degraded,
                        detail: Some("MQTT message rejected or adapter degraded".into()),
                        observed_at: Utc::now(),
                    });
                }
            }
        })
        .expect("spawn MQTT ingest thread");
}

/// Matter reste optionnel : un socket ou un sidecar indisponible dégrade
/// uniquement son adaptateur. Le jeton de socket ne vient jamais de SQLite ni
/// d'une variable de diagnostic ; l'outil d'installation le place au préalable
/// dans le trousseau sous `matter:local-rpc`.
fn start_configured_matter(
    service: HomeService,
    dispatcher: Arc<SwitchingCommandDispatcher>,
    secrets: Arc<dyn SecretStore>,
) -> Option<Arc<LocalMatterClient>> {
    let Ok(socket_path) = env::var("ROBINE_MATTER_SOCKET") else {
        return None;
    };
    let sidecar = env::var_os("ROBINE_MATTERD_BIN").map(|binary| {
        let sidecar = Arc::new(Mutex::new(None));
        ensure_matter_sidecar(&sidecar, &binary, &socket_path);
        (sidecar, binary)
    });
    let token = match secrets.get("matter:local-rpc") {
        Ok(Some(token)) => token,
        _ => {
            let _ = service.update_adapter_health(robine_domain::AdapterHealth {
                adapter_id: robine_domain::AdapterId::new(MATTER_ADAPTER_ID)
                    .expect("static adapter id"),
                status: robine_domain::AdapterStatus::Degraded,
                detail: Some("Matter sidecar authorization is unavailable".into()),
                observed_at: Utc::now(),
            });
            return None;
        }
    };
    let client = match LocalMatterClient::new(
        PathBuf::from(&socket_path),
        token,
        std::time::Duration::from_secs(5),
    ) {
        Ok(client) => Arc::new(client),
        Err(_) => return None,
    };
    let matter_dispatcher = Arc::new(MatterCommandDispatcher::new(client.clone()));
    if dispatcher.register(matter_dispatcher.clone()).is_err() {
        return None;
    }
    let adapter = Arc::new(MatterAdapter::new(
        client.clone(),
        service.clone(),
        matter_dispatcher,
    ));
    if let Err(error) = adapter.synchronize(Utc::now()) {
        tracing::warn!(error = %error, "Matter sidecar initial synchronization failed");
        let _ = service.update_adapter_health(robine_domain::AdapterHealth {
            adapter_id: robine_domain::AdapterId::new(MATTER_ADAPTER_ID)
                .expect("static adapter id"),
            status: robine_domain::AdapterStatus::Degraded,
            detail: Some("Matter sidecar initial synchronization failed".into()),
            observed_at: Utc::now(),
        });
    }
    std::thread::Builder::new()
        .name("robine-matter-supervisor".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if let Some((sidecar, binary)) = &sidecar {
                ensure_matter_sidecar(sidecar, binary, &socket_path);
            }
            if let Err(error) = adapter.synchronize(Utc::now()) {
                tracing::warn!(error = %error, "Matter sidecar synchronization failed; retrying");
                let _ = service.update_adapter_health(robine_domain::AdapterHealth {
                    adapter_id: robine_domain::AdapterId::new(MATTER_ADAPTER_ID)
                        .expect("static adapter id"),
                    status: robine_domain::AdapterStatus::Degraded,
                    detail: Some("Matter sidecar synchronization failed; retrying".into()),
                    observed_at: Utc::now(),
                });
            }
        })
        .expect("spawn Matter supervisor");
    Some(client)
}

/// Lance seulement un binaire explicitement configuré. Le secret RPC n'est
/// pas passé en argument ni dans l'environnement : `robine-matterd` le lit
/// lui-même depuis le trousseau système.
fn ensure_matter_sidecar(
    child: &Arc<Mutex<Option<Child>>>,
    binary: &std::ffi::OsString,
    socket_path: &str,
) {
    let Ok(mut child) = child.lock() else {
        return;
    };
    let running = child
        .as_mut()
        .is_some_and(|process| process.try_wait().ok().flatten().is_none());
    if running {
        return;
    }
    match ProcessCommand::new(binary)
        .env("ROBINE_MATTER_SOCKET", socket_path)
        .spawn()
    {
        Ok(process) => *child = Some(process),
        Err(error) => tracing::warn!(error = %error, "Matter sidecar could not be started"),
    }
}

/// Consomme les changements déjà publiés par le store et exécute les Flows
/// correspondants. Les commandes générées repassent par `HomeService`, donc un
/// Flow ne peut pas contourner les adaptateurs ni leurs contrôles de capacité.
fn spawn_automation_engine(store: Arc<SqliteStore>, service: HomeService) {
    let flows = FlowService::new(store.clone(), store.clone());
    actix_web::rt::spawn(async move {
        let mut events = store.subscribe_events();
        let mut cursor = match store.automation_engine_cursor() {
            Ok(Some(cursor)) => cursor,
            Ok(None) => match store.latest_event_sequence() {
                Ok(cursor) => {
                    if let Err(error) = store.save_automation_engine_cursor(cursor) {
                        tracing::warn!(error = %error, "automation engine could not initialize its cursor");
                    }
                    cursor
                }
                Err(error) => {
                    tracing::warn!(error = %error, "automation engine could not read the event journal");
                    0
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, "automation engine could not restore its cursor");
                0
            }
        };
        replay_automation_events(&store, &flows, &service, &mut cursor).await;
        let mut scheduler = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = scheduler.tick() => {
                    let flows = flows.clone();
                    let service = service.clone();
                    let _ = actix_web::rt::task::spawn_blocking(move || {
                        let now = Utc::now();
                        for execution in flows.resume_due(&service, now) {
                            if let Err(error) = execution { tracing::warn!(error = %error, "Flow resume failed"); }
                        }
                        for execution in flows.execute_scheduled(&service, now) {
                            if let Err(error) = execution { tracing::warn!(error = %error, "Flow schedule execution failed"); }
                        }
                    }).await;
                }
                event = events.recv() => match event {
                    Ok(event) => {
                        if event.sequence <= cursor { continue; }
                        execute_automation_event(flows.clone(), service.clone(), event.clone()).await;
                        cursor = event.sequence;
                        if let Err(error) = store.save_automation_engine_cursor(cursor) {
                            tracing::warn!(error = %error, cursor, "automation engine could not save its cursor");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        tracing::warn!(count, "automation engine is resynchronizing after event lag");
                        replay_automation_events(&store, &flows, &service, &mut cursor).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    });
}

async fn replay_automation_events(
    store: &Arc<SqliteStore>,
    flows: &FlowService,
    service: &HomeService,
    cursor: &mut u64,
) {
    loop {
        let events = match store.events_after(*cursor, 100) {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(error = %error, cursor = *cursor, "automation engine could not replay the event journal");
                return;
            }
        };
        if events.is_empty() {
            return;
        }
        for event in events {
            execute_automation_event(flows.clone(), service.clone(), event.clone()).await;
            *cursor = event.sequence;
            if let Err(error) = store.save_automation_engine_cursor(*cursor) {
                tracing::warn!(error = %error, cursor = *cursor, "automation engine could not save its replay cursor");
                return;
            }
        }
    }
}

async fn execute_automation_event(
    flows: FlowService,
    service: HomeService,
    event: robine_domain::EventEnvelope,
) {
    let _ = actix_web::rt::task::spawn_blocking(move || {
        if let robine_domain::EventData::StateReported { state } = &event.data {
            for execution in flows.execute_state_triggered(state, &service, Utc::now()) {
                if let Err(error) = execution {
                    tracing::warn!(error = %error, "Flow state execution failed");
                }
            }
        }
        for execution in flows.execute_event_triggered(&event, &service, Utc::now()) {
            if let Err(error) = execution {
                tracing::warn!(error = %error, "Flow event execution failed");
            }
        }
    })
    .await;
}

/// Les adaptateurs confirment par un état rapporté. Sans ce rapport, une
/// commande ne reste pas ambiguë indéfiniment : le journal publie un état
/// `command.expired` récupérable par les apps après un délai borné.
fn spawn_command_expirer(service: HomeService) {
    actix_web::rt::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let service = service.clone();
            let _ = actix_web::rt::task::spawn_blocking(move || {
                service.expire_stale_commands(Utc::now(), chrono::Duration::seconds(30))
            })
            .await;
        }
    });
}

/// La compaction ne fait jamais partie du chemin de mutation. Chaque passage
/// retire de petits lots d'historiques expirés afin de préserver la latence du
/// writer, puis reprendra au tick suivant si une base a beaucoup d'ancienneté.
fn spawn_retention_pruner(store: Arc<SqliteStore>) {
    actix_web::rt::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
        loop {
            ticker.tick().await;
            let store = store.clone();
            let _ = actix_web::rt::task::spawn_blocking(move || {
                match store.prune_retained_data(Utc::now(), 500) {
                    Ok(purged) if !purged.is_empty() => tracing::info!(
                        events = purged.events,
                        flow_run_traces = purged.flow_run_traces,
                        flow_trigger_claims = purged.flow_trigger_claims,
                        "retention compaction removed expired history"
                    ),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(error = %error, "retention compaction failed"),
                }
            })
            .await;
        }
    });
}

struct RuntimeMcpAuthenticator {
    store: Arc<SqliteStore>,
}

struct RuntimeMatterAdministration {
    client: Arc<LocalMatterClient>,
}

struct RuntimeBackupAdministration {
    database: PathBuf,
    backups: PathBuf,
}

impl BackupAdministration for RuntimeBackupAdministration {
    fn create_backup(&self) -> Result<robine_api_contract::BackupResponse, ApplicationError> {
        let manifest = robine_store_backup::create_snapshot(&self.database, &self.backups)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        Ok(robine_api_contract::BackupResponse {
            manifest_version: manifest.manifest_version,
            created_at: manifest.created_at.to_rfc3339(),
            database_file: manifest.database_file,
            bytes: manifest.bytes,
            sha256: manifest.sha256,
        })
    }
}

impl MatterAdministration for RuntimeMatterAdministration {
    fn start_commissioning(&self, setup_code: String) -> Result<String, MatterAdministrationError> {
        self.client
            .start_commissioning(&setup_code)
            .map_err(|_| MatterAdministrationError::Unavailable)
    }

    fn commissioning_job(
        &self,
        job_id: String,
    ) -> Result<robine_matter_contract::CommissioningJob, MatterAdministrationError> {
        self.client
            .commissioning_job(&job_id)
            .map_err(|error| match error {
                robine_integration_matter::MatterError::Protocol(detail)
                    if detail.contains("not found") =>
                {
                    MatterAdministrationError::JobNotFound
                }
                _ => MatterAdministrationError::Unavailable,
            })
    }
}

impl McpAuthenticator for RuntimeMcpAuthenticator {
    fn authenticate(&self, bearer: &str) -> Result<McpPrincipal, AuthenticationError> {
        self.store
            .authenticate_mcp(bearer, Utc::now())
            .ok()
            .flatten()
            .map(|identity| McpPrincipal {
                token_id: identity.token_id,
                scopes: Scopes::new(
                    identity
                        .scopes
                        .iter()
                        .filter_map(|scope| Scope::from_storage(scope)),
                ),
                write_policy: identity.write_policy,
            })
            .ok_or(AuthenticationError::Invalid)
    }
}

struct RuntimeMcpApprovalAuthorizer {
    store: Arc<SqliteStore>,
}

impl McpApprovalAuthorizer for RuntimeMcpApprovalAuthorizer {
    fn consume(
        &self,
        principal: &McpPrincipal,
        tool: &str,
        arguments_hash: &str,
        approval_id: &str,
    ) -> Result<bool, ApprovalError> {
        self.store
            .consume_mcp_approval(
                &principal.token_id,
                tool,
                arguments_hash,
                approval_id,
                Utc::now(),
            )
            .map_err(|_| ApprovalError::Unavailable)
    }

    fn claim_allow_listed_command(
        &self,
        principal: &McpPrincipal,
        tool: &str,
        arguments_hash: &str,
        max_commands_per_hour: u32,
    ) -> Result<bool, ApprovalError> {
        self.store
            .claim_mcp_allow_listed_command(
                &principal.token_id,
                tool,
                arguments_hash,
                max_commands_per_hour,
                Utc::now(),
            )
            .map_err(|_| ApprovalError::Unavailable)
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let mode = runtime_mode(env::args_os().skip(1))?;
    let data_dir = env::var_os("ROBINE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));
    std::fs::create_dir_all(&data_dir).context("creating Robine data directory")?;
    if let RuntimeMode::Restore { manifest } = mode {
        return restore_from_manifest(&data_dir, manifest);
    }
    let _runtime_lock = RuntimeDataLock::acquire(&data_dir)?;
    let database_path = data_dir.join("robine.sqlite3");
    let store = Arc::new(SqliteStore::open(&database_path)?);
    let dispatcher = Arc::new(SwitchingCommandDispatcher::default());
    let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
    let bind = env::var("ROBINE_BIND").unwrap_or_else(|_| "127.0.0.1:3030".into());
    let tls = tls_for_listener(
        &bind,
        env::var_os("ROBINE_TLS_CERT").map(PathBuf::from),
        env::var_os("ROBINE_TLS_KEY").map(PathBuf::from),
    )?;
    let web_dir = env::var_os("ROBINE_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("apps/web/dist"));
    let secrets: Arc<dyn SecretStore> =
        Arc::new(MacOsKeychainSecretStore::new("io.robine.server")?);
    let hue = Arc::new(HueRuntimeAdministration::new(
        service.clone(),
        store.clone(),
        dispatcher.clone(),
        secrets.clone(),
    ));
    hue.restore_paired_bridges();
    start_configured_mqtt(service.clone(), dispatcher.clone(), secrets.clone());
    let matter = start_configured_matter(service.clone(), dispatcher.clone(), secrets.clone());
    spawn_automation_engine(store.clone(), service.clone());
    spawn_command_expirer(service.clone());
    spawn_retention_pruner(store.clone());
    let mut api_state = ServerState::new(service.clone(), store.clone())
        .with_hue(hue)
        .with_backups(Arc::new(RuntimeBackupAdministration {
            database: database_path,
            backups: data_dir.join("backups"),
        }));
    if let Some(client) = matter {
        api_state = api_state.with_matter(Arc::new(RuntimeMatterAdministration { client }));
    }
    let mcp_state = McpHttpState::new(
        McpTools::new(service, FlowService::new(store.clone(), store.clone())),
        Arc::new(RuntimeMcpAuthenticator {
            store: store.clone(),
        }),
        Arc::new(RuntimeMcpApprovalAuthorizer {
            store: store.clone(),
        }),
        std::iter::empty(),
    );
    let tls_enabled = tls.is_some();
    let server = HttpServer::new(move || {
        let mut security_headers = DefaultHeaders::new()
            .add((
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self' 'wasm-unsafe-eval'",
            ))
            .add((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .add((header::REFERRER_POLICY, "no-referrer"));
        if tls_enabled {
            security_headers = security_headers.add((
                header::STRICT_TRANSPORT_SECURITY,
                "max-age=31536000; includeSubDomains",
            ));
        }
        App::new()
            .wrap(security_headers)
            .app_data(web::Data::new(api_state.clone()))
            .configure(configure_api)
            .configure(|configuration| configure_mcp(configuration, mcp_state.clone()))
            .service(Files::new("/", web_dir.clone()).index_file("index.html"))
    });
    let server = match tls {
        Some(configuration) => server.bind_rustls_0_23(&bind, configuration)?,
        None => server.bind(&bind)?,
    };
    server.run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_domain::{AdapterId, Capability, DeviceDiscovery, DiscoveryEntity};
    use robine_secret_store::MemorySecretStore;

    #[test]
    fn restore_mode_requires_an_explicit_manifest_and_confirmation() {
        assert!(runtime_mode([std::ffi::OsString::from("restore")]).is_err());
        assert!(
            runtime_mode([
                std::ffi::OsString::from("restore"),
                std::ffi::OsString::from("--manifest"),
                std::ffi::OsString::from("backup.manifest.json"),
            ])
            .is_err()
        );
        assert_eq!(
            runtime_mode([
                std::ffi::OsString::from("restore"),
                std::ffi::OsString::from("--manifest"),
                std::ffi::OsString::from("backup.manifest.json"),
                std::ffi::OsString::from("--confirm"),
            ])
            .unwrap(),
            RuntimeMode::Restore {
                manifest: PathBuf::from("backup.manifest.json"),
            }
        );
    }

    #[test]
    fn maintenance_restore_verifies_a_manifest_and_keeps_the_previous_database() {
        let root =
            std::env::temp_dir().join(format!("robine-runtime-restore-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        let backups = data.join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let database = data.join("robine.sqlite3");
        drop(SqliteStore::open(&database).unwrap());
        let manifest = robine_store_backup::create_snapshot(&database, &backups).unwrap();
        let stem = std::path::Path::new(&manifest.database_file)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        restore_from_manifest(&data, backups.join(format!("{stem}.manifest.json"))).unwrap();
        assert!(database.exists());
        assert!(std::fs::read_dir(&data).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("robine-pre-restore-")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plaintext_is_restricted_to_loopback() {
        assert!(
            tls_for_listener("127.0.0.1:3030", None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            tls_for_listener("[::1]:3030", None, None)
                .unwrap()
                .is_none()
        );
        assert!(tls_for_listener("0.0.0.0:3030", None, None).is_err());
        assert!(tls_for_listener("robine.local:3030", None, None).is_err());
    }

    #[test]
    fn certificate_and_key_must_be_configured_together() {
        assert!(tls_for_listener("127.0.0.1:3030", Some("cert.pem".into()), None).is_err());
        assert!(tls_for_listener("127.0.0.1:3030", None, Some("key.pem".into())).is_err());
    }

    #[test]
    fn repeated_hue_room_import_resolves_to_the_existing_area() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(SwitchingCommandDispatcher::default());
        let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let hue = HueRuntimeAdministration::new(
            service.clone(),
            store,
            dispatcher,
            Arc::new(MemorySecretStore::default()),
        );
        let device = service
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                    protocol_address: "light-a".into(),
                    name: "Lampe salon".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-a".into(),
                        name: "Lampe salon".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let suggestion = HueRoomSuggestion {
            name: "Salon".into(),
            entity_ids: vec![device.entities[0].id.clone()],
        };
        let area = service
            .create_area_with_entities(
                suggestion.name.clone(),
                suggestion.entity_ids.clone(),
                Utc::now(),
            )
            .unwrap();
        assert_eq!(hue.existing_imported_room(&suggestion).unwrap(), Some(area));
    }

    #[actix_web::test]
    async fn automation_engine_replays_persisted_events_and_advances_its_cursor() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(SwitchingCommandDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                "(flow (on (event :type \"area.created\")) (do (audit :message \"replayed\")))"
                    .into(),
                true,
                Utc::now(),
            )
            .unwrap();
        service.create_area("Entrée".into(), Utc::now()).unwrap();

        let mut cursor = 0;
        replay_automation_events(&store, &flows, &service, &mut cursor).await;
        assert!(cursor > 0);
        assert_eq!(store.automation_engine_cursor().unwrap(), Some(cursor));
        let runs = flows.list_runs(&flow.id, 20).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].result["status"], "completed");

        replay_automation_events(&store, &flows, &service, &mut cursor).await;
        assert_eq!(flows.list_runs(&flow.id, 20).unwrap().len(), 1);
    }
}
