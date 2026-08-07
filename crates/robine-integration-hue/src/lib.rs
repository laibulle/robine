//! Adaptateur Philips Hue. Les types de l'API Hue restent à cette frontière.

use chrono::{DateTime, Utc};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::*;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct HueLight {
    pub resource_id: String,
    pub name: String,
    pub on: bool,
    /// Valeur Hue 0..100, normalisée en pourcentage avant le domaine.
    pub brightness: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HueInventory {
    pub bridge_id: String,
    pub lights: Vec<HueLight>,
}

/// Candidat d'appairage publié par Bonjour/mDNS. La découverte ne contacte
/// aucun service Internet et ne contient aucun secret ni certificat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HueBridgeCandidate {
    pub name: String,
    pub host: String,
    pub addresses: Vec<String>,
}

/// Cherche les bridges qui annoncent le service local Hue pendant une fenêtre
/// bornée. L'adresse est laissée sans port : l'association Hue V1 utilise
/// HTTPS (443), indépendamment du port éventuellement annoncé par mDNS.
pub fn discover_bridges(timeout: std::time::Duration) -> Result<Vec<HueBridgeCandidate>, HueError> {
    let daemon = ServiceDaemon::new().map_err(|error| HueError::Transport(error.to_string()))?;
    let receiver = daemon
        .browse("_hue._tcp.local.")
        .map_err(|error| HueError::Transport(error.to_string()))?;
    let deadline = std::time::Instant::now() + timeout;
    let mut candidates = HashMap::new();
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(event) = receiver.recv_timeout(remaining) else {
            break;
        };
        if let ServiceEvent::ServiceResolved(info) = event {
            let mut addresses: Vec<String> =
                info.addresses.iter().map(ToString::to_string).collect();
            addresses.sort();
            addresses.dedup();
            if !addresses.is_empty() {
                candidates.insert(
                    info.fullname.clone(),
                    HueBridgeCandidate {
                        name: info.fullname,
                        host: info.host,
                        addresses,
                    },
                );
            }
        }
    }
    daemon
        .shutdown()
        .map_err(|error| HueError::Transport(error.to_string()))?;
    let mut candidates: Vec<_> = candidates.into_values().collect();
    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(candidates)
}

#[derive(Clone, Debug, PartialEq)]
pub enum HueValue {
    On(bool),
    Brightness(f64),
}

/// Mise à jour partielle normalisée depuis l'EventStream Hue v2.
#[derive(Clone, Debug, PartialEq)]
pub struct HueLightStateUpdate {
    pub resource_id: String,
    pub on: Option<bool>,
    pub brightness: Option<f64>,
    pub source_at: Option<DateTime<Utc>>,
}

pub trait HueBridgeClient: Send + Sync {
    fn create_application_key(&self) -> Result<String, HueError>;
    fn inventory(&self) -> Result<HueInventory, HueError>;
    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError>;
}

#[derive(Debug, Error)]
pub enum HueError {
    #[error("the bridge button must be pressed before pairing")]
    LinkButtonNotPressed,
    #[error("bridge request failed: {0}")]
    Transport(String),
    #[error("unsupported Hue command")]
    UnsupportedCommand,
    #[error("the bridge response is malformed: {0}")]
    InvalidResponse(String),
    #[error("an application key is required for this Hue request")]
    MissingApplicationKey,
    #[error("Hue rejected the request: {0}")]
    Rejected(String),
}

/// Client de bridge local Hue. Il ne désactive jamais la validation TLS : le
/// certificat du bridge doit être fourni par le parcours d'association, après
/// vérification de son empreinte par l'administrateur.
pub struct HueHttpBridgeClient {
    client: reqwest::blocking::Client,
    base_url: String,
    application_key: Option<String>,
}

impl HueHttpBridgeClient {
    pub fn with_pinned_certificate(
        authority: &str,
        certificate_pem: &[u8],
        application_key: Option<String>,
    ) -> Result<Self, HueError> {
        let authority = authority.trim().trim_matches('/');
        if authority.is_empty() || authority.contains('/') {
            return Err(HueError::Transport("invalid bridge authority".into()));
        }
        let certificate = reqwest::Certificate::from_pem(certificate_pem)
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let client = reqwest::blocking::Client::builder()
            .add_root_certificate(certificate)
            .build()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            base_url: format!("https://{authority}"),
            application_key,
        })
    }

    fn v2_get(&self, path: &str) -> Result<Value, HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .get(format!("{}{path}", self.base_url))
            .header("hue-application-key", application_key)
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))
    }

    fn v2_put(&self, path: &str, payload: Value) -> Result<(), HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .put(format!("{}{path}", self.base_url))
            .header("hue-application-key", application_key)
            .json(&payload)
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let response: Value = response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
        if let Some(errors) = response.get("errors").and_then(Value::as_array) {
            if !errors.is_empty() {
                return Err(HueError::Rejected(
                    serde_json::to_string(errors).unwrap_or_else(|_| "unknown Hue error".into()),
                ));
            }
        }
        Ok(())
    }

    fn bridge_id(&self) -> Result<String, HueError> {
        let response = self.v2_get("/clip/v2/resource/bridge")?;
        response
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| HueError::InvalidResponse("bridge identifier is missing".into()))
    }

    /// Lit le flux SSE Hue dans un thread dédié. Le callback ne reçoit que des
    /// mises à jour normalisées ; ni le payload brut ni la clé d'application ne
    /// quittent l'infrastructure.
    pub fn listen_events(
        &self,
        mut on_updates: impl FnMut(Vec<HueLightStateUpdate>) -> Result<(), HueError>,
    ) -> Result<(), HueError> {
        let application_key = self
            .application_key
            .as_deref()
            .ok_or(HueError::MissingApplicationKey)?;
        let response = self
            .client
            .get(format!("{}/eventstream/clip/v2", self.base_url))
            .header("hue-application-key", application_key)
            .header("accept", "text/event-stream")
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let mut data = String::new();
        for line in BufReader::new(response).lines() {
            let line = line.map_err(|error| HueError::Transport(error.to_string()))?;
            if let Some(fragment) = line.strip_prefix("data:") {
                data.push_str(fragment.trim_start());
                continue;
            }
            if line.is_empty() && !data.is_empty() {
                let updates = parse_v2_event(&data)?;
                data.clear();
                if !updates.is_empty() {
                    on_updates(updates)?;
                }
            }
        }
        Err(HueError::Transport("Hue event stream closed".into()))
    }
}

impl HueBridgeClient for HueHttpBridgeClient {
    fn create_application_key(&self) -> Result<String, HueError> {
        let response = self
            .client
            .post(format!("{}/api", self.base_url))
            .json(&json!({ "devicetype": "robine#server" }))
            .send()
            .map_err(|error| HueError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| HueError::Transport(error.to_string()))?;
        let response: Value = response
            .json()
            .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
        let Some(entry) = response.as_array().and_then(|items| items.first()) else {
            return Err(HueError::InvalidResponse(
                "pairing response is not an array".into(),
            ));
        };
        if let Some(error) = entry.get("error") {
            if error.get("type").and_then(Value::as_i64) == Some(101) {
                return Err(HueError::LinkButtonNotPressed);
            }
            return Err(HueError::Rejected(error.to_string()));
        }
        entry
            .get("success")
            .and_then(|success| success.get("username"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                HueError::InvalidResponse("pairing response has no application key".into())
            })
    }

    fn inventory(&self) -> Result<HueInventory, HueError> {
        let bridge_id = self.bridge_id()?;
        let response = self.v2_get("/clip/v2/resource/light")?;
        let lights = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| HueError::InvalidResponse("light inventory has no data array".into()))?
            .iter()
            .map(|light| {
                let resource_id = light
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| HueError::InvalidResponse("light has no id".into()))?
                    .to_owned();
                let name = light
                    .pointer("/metadata/name")
                    .and_then(Value::as_str)
                    .unwrap_or("Philips Hue light")
                    .to_owned();
                let on = light
                    .pointer("/on/on")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let brightness = light
                    .pointer("/dimming/brightness")
                    .and_then(Value::as_f64)
                    .map(|value| value.clamp(0.0, 100.0));
                Ok(HueLight {
                    resource_id,
                    name,
                    on,
                    brightness,
                })
            })
            .collect::<Result<Vec<_>, HueError>>()?;
        Ok(HueInventory { bridge_id, lights })
    }

    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError> {
        let payload = match value {
            HueValue::On(on) => json!({ "on": { "on": on } }),
            HueValue::Brightness(brightness) => {
                json!({ "dimming": { "brightness": brightness.clamp(0.0, 100.0) } })
            }
        };
        self.v2_put(&format!("/clip/v2/resource/light/{resource_id}"), payload)
    }
}

/// Décode un événement `data:` complet provenant du flux SSE Hue v2. Le
/// lecteur réseau garde les frontières de trame SSE ; ce parseur reste pur et
/// testable sans bridge.
pub fn parse_v2_event(payload: &str) -> Result<Vec<HueLightStateUpdate>, HueError> {
    let events: Value = serde_json::from_str(payload)
        .map_err(|error| HueError::InvalidResponse(error.to_string()))?;
    let events = events
        .as_array()
        .ok_or_else(|| HueError::InvalidResponse("event payload is not an array".into()))?;
    let mut updates = Vec::new();
    for event in events {
        for resource in event
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if resource.get("type").and_then(Value::as_str) != Some("light") {
                continue;
            }
            let Some(resource_id) = resource.get("id").and_then(Value::as_str) else {
                continue;
            };
            let on = resource.pointer("/on/on").and_then(Value::as_bool);
            let brightness = resource
                .pointer("/dimming/brightness")
                .and_then(Value::as_f64)
                .map(|value| value.clamp(0.0, 100.0));
            let source_at = event
                .get("creationtime")
                .and_then(Value::as_str)
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc));
            if on.is_some() || brightness.is_some() {
                updates.push(HueLightStateUpdate {
                    resource_id: resource_id.into(),
                    on,
                    brightness,
                    source_at,
                });
            }
        }
    }
    Ok(updates)
}

/// Pont vers un client réel ou un faux. Il ne stocke jamais la clé Hue : le
/// runtime l'enverra à un magasin de secrets lors de l'intégration réseau.
pub struct HueAdapter<C: HueBridgeClient> {
    client: Arc<C>,
    service: HomeService,
    dispatcher: Option<Arc<HueCommandDispatcher<C>>>,
    routes: Mutex<HashMap<String, EntityId>>,
}

impl<C: HueBridgeClient> HueAdapter<C> {
    pub fn new(client: Arc<C>, service: HomeService) -> Self {
        Self {
            client,
            service,
            dispatcher: None,
            routes: Mutex::new(HashMap::new()),
        }
    }
    pub fn with_dispatcher(
        client: Arc<C>,
        service: HomeService,
        dispatcher: Arc<HueCommandDispatcher<C>>,
    ) -> Self {
        Self {
            client,
            service,
            dispatcher: Some(dispatcher),
            routes: Mutex::new(HashMap::new()),
        }
    }
    pub fn pair(&self) -> Result<String, HueError> {
        self.client.create_application_key()
    }
    pub fn synchronize(&self, now: DateTime<Utc>) -> Result<Vec<Device>, ApplicationError> {
        let inventory = self
            .client
            .inventory()
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        let adapter_id = AdapterId::new(format!("hue:{}", inventory.bridge_id))?;
        let devices = inventory
            .lights
            .into_iter()
            .map(|light| {
                let discovery = DeviceDiscovery {
                    adapter_id: adapter_id.clone(),
                    protocol_address: light.resource_id.clone(),
                    name: light.name.clone(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: light.resource_id.clone(),
                        name: light.name,
                        kind: "light".into(),
                        capabilities: vec![
                            Capability::new("switch", 1)?,
                            Capability::new("light.brightness", 1)?,
                        ],
                    }],
                };
                let device = self.service.register_discovery(discovery, now)?;
                let entity_id = device.entities[0].id.clone();
                self.routes
                    .lock()
                    .map_err(|_| {
                        ApplicationError::Infrastructure("Hue routing mutex poisoned".into())
                    })?
                    .insert(light.resource_id.clone(), entity_id.clone());
                if let Some(dispatcher) = &self.dispatcher {
                    dispatcher.register_light(entity_id.clone(), light.resource_id);
                }
                self.service.apply_reported_state(
                    ReportedState {
                        entity_id: entity_id.clone(),
                        key: "switch".into(),
                        value: StateValue::Bool(light.on),
                        source_at: now,
                    },
                    now,
                )?;
                if let Some(value) = light.brightness {
                    self.service.apply_reported_state(
                        ReportedState {
                            entity_id,
                            key: "light.brightness".into(),
                            value: StateValue::Percentage(value.clamp(0.0, 100.0)),
                            source_at: now,
                        },
                        now,
                    )?;
                }
                Ok(device)
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        self.service.update_adapter_health(AdapterHealth {
            adapter_id,
            status: AdapterStatus::Available,
            detail: None,
            observed_at: now,
        })?;
        Ok(devices)
    }

    pub fn apply_light_update(
        &self,
        update: HueLightStateUpdate,
        received_at: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        let entity_id = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
            .get(&update.resource_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("unknown Hue light event".into()))?;
        let source_at = update.source_at.unwrap_or(received_at);
        if let Some(on) = update.on {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "switch".into(),
                    value: StateValue::Bool(on),
                    source_at,
                },
                received_at,
            )?;
        }
        if let Some(brightness) = update.brightness {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id,
                    key: "light.brightness".into(),
                    value: StateValue::Percentage(brightness),
                    source_at,
                },
                received_at,
            )?;
        }
        Ok(())
    }
}

impl HueAdapter<HueHttpBridgeClient> {
    /// À appeler depuis la tâche supervisée du runtime après une synchronisation
    /// réussie. Un retour est une rupture de flux, à traiter par le backoff du
    /// superviseur avant une nouvelle synchronisation.
    pub fn listen_for_events(&self) -> Result<(), ApplicationError> {
        self.client
            .listen_events(|updates| {
                let received_at = Utc::now();
                for update in updates {
                    self.apply_light_update(update, received_at)
                        .map_err(|error| HueError::Transport(error.to_string()))?;
                }
                Ok(())
            })
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))
    }
}

/// Le dispatcher est séparé de l'adaptateur pour pouvoir être enregistré au
/// démarrage du runtime. La table est peuplée par la synchronisation Hue.
#[derive(Default)]
pub struct HueCommandDispatcher<C: HueBridgeClient> {
    client: Option<Arc<C>>,
    routes: Mutex<HashMap<EntityId, String>>,
}

impl<C: HueBridgeClient> HueCommandDispatcher<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client: Some(client),
            routes: Mutex::new(HashMap::new()),
        }
    }
    pub fn register_light(&self, entity_id: EntityId, hue_resource_id: String) {
        self.routes
            .lock()
            .expect("hue route mutex poisoned")
            .insert(entity_id, hue_resource_id);
    }
}

impl<C: HueBridgeClient> CommandDispatcher for HueCommandDispatcher<C> {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let resource_id = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Hue routing mutex poisoned".into()))?
            .get(&command.entity_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("no Hue route for entity".into()))?;
        let value = match (command.key.as_str(), command.value) {
            ("switch", StateValue::Bool(value)) => HueValue::On(value),
            ("light.brightness", StateValue::Percentage(value)) => {
                HueValue::Brightness(value.clamp(0.0, 100.0))
            }
            _ => {
                return Err(ApplicationError::Infrastructure(
                    HueError::UnsupportedCommand.to_string(),
                ));
            }
        };
        self.client
            .as_ref()
            .expect("Hue dispatcher has a client")
            .set_light_state(&resource_id, value)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))
    }
}

/// Faux déterministe employé en CI ; aucune requête réseau ne lui est associée.
#[derive(Default)]
pub struct FakeHueBridge {
    pub button_pressed: Mutex<bool>,
    pub inventory: Mutex<HueInventory>,
    pub commands: Mutex<Vec<(String, HueValue)>>,
}

impl FakeHueBridge {
    pub fn with_inventory(inventory: HueInventory) -> Self {
        Self {
            inventory: Mutex::new(inventory),
            ..Self::default()
        }
    }
}

impl HueBridgeClient for FakeHueBridge {
    fn create_application_key(&self) -> Result<String, HueError> {
        if *self.button_pressed.lock().expect("fake hue mutex poisoned") {
            Ok("fake-hue-application-key".into())
        } else {
            Err(HueError::LinkButtonNotPressed)
        }
    }
    fn inventory(&self) -> Result<HueInventory, HueError> {
        Ok(self
            .inventory
            .lock()
            .expect("fake hue mutex poisoned")
            .clone())
    }
    fn set_light_state(&self, resource_id: &str, value: HueValue) -> Result<(), HueError> {
        self.commands
            .lock()
            .expect("fake hue mutex poisoned")
            .push((resource_id.into(), value));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::HomeRepository;
    use robine_store_sqlite::SqliteStore;

    #[test]
    fn synchronization_keeps_identity_and_commands_the_mapped_hue_light() {
        let bridge = Arc::new(FakeHueBridge::with_inventory(HueInventory {
            bridge_id: "bridge-a".into(),
            lights: vec![HueLight {
                resource_id: "light-a".into(),
                name: "Lampe du salon".into(),
                on: true,
                brightness: Some(42.0),
            }],
        }));
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(HueCommandDispatcher::new(bridge.clone()));
        let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let adapter = HueAdapter::with_dispatcher(bridge.clone(), service.clone(), dispatcher);
        let now = Utc::now();

        let first = adapter.synchronize(now).unwrap();
        let second = adapter.synchronize(now).unwrap();
        assert_eq!(first[0].id, second[0].id);
        assert_eq!(first[0].entities[0].id, second[0].entities[0].id);
        assert!(
            store
                .list_adapter_health()
                .unwrap()
                .iter()
                .any(|health| health.status == AdapterStatus::Available)
        );

        let command = service
            .request_command(
                first[0].entities[0].id.clone(),
                "switch".into(),
                StateValue::Bool(false),
                "toggle-1".into(),
                now,
            )
            .unwrap();
        assert_eq!(command.idempotency_key, "toggle-1");
        assert_eq!(
            *bridge.commands.lock().unwrap(),
            vec![("light-a".into(), HueValue::On(false))]
        );
        let command_id = command.id.clone();
        adapter
            .apply_light_update(
                HueLightStateUpdate {
                    resource_id: "light-a".into(),
                    on: Some(false),
                    brightness: None,
                    source_at: Some(now),
                },
                now,
            )
            .unwrap();
        assert!(
            store
                .events_after(0, 20)
                .unwrap()
                .iter()
                .any(|event| matches!(event.data, EventData::StateReported { .. }))
        );
        assert!(store.events_after(0, 20).unwrap().iter().any(
            |event| matches!(&event.data, EventData::CommandConfirmed { command } if command.id == command_id)
        ));
    }

    #[test]
    fn parses_partial_light_updates_from_hue_v2_eventstream() {
        let updates = parse_v2_event(
            r#"[{"type":"update","data":[{"type":"light","id":"light-a","on":{"on":false}},{"type":"light","id":"light-b","dimming":{"brightness":17.5}},{"type":"device","id":"ignored"}]}]"#,
        )
        .unwrap();
        assert_eq!(
            updates,
            vec![
                HueLightStateUpdate {
                    resource_id: "light-a".into(),
                    on: Some(false),
                    brightness: None,
                    source_at: None,
                },
                HueLightStateUpdate {
                    resource_id: "light-b".into(),
                    on: None,
                    brightness: Some(17.5),
                    source_at: None,
                },
            ]
        );
    }
}
