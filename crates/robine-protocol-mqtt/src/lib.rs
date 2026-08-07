//! Adaptateur MQTT borné. Les topics et payloads ne traversent jamais cette
//! frontière vers le domaine ni les cas d'utilisation.

use chrono::{DateTime, Utc};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::*;
use robine_mqtt_compat_ha::HaStateEncoding;
use robine_mqtt_contract::{DiscoveryConfig, Profile, discovery_topic};
use rumqttc::{Client, Event, MqttOptions, Packet, QoS, Transport};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};
use thiserror::Error;

pub const ADAPTER_ID: &str = "mqtt:local";
pub const MAX_STATE_BYTES: usize = 16 * 1024;

pub trait MqttPublisher: Send + Sync {
    fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<(), MqttError>;
    fn subscribe(&self, _topic: &str) -> Result<(), MqttError> {
        Ok(())
    }
}

/// Matériel TLS injecté par le runtime depuis un magasin de secrets. Il ne
/// transite jamais vers le domaine, SQLite, les sauvegardes ou les diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MqttTlsConfiguration {
    /// Vérifie le broker avec les racines de confiance du système.
    SystemRoots,
    /// Autorité privée et, facultativement, authentification mutuelle mTLS.
    Custom {
        ca_certificate_pem: Vec<u8>,
        client_auth: Option<(Vec<u8>, Vec<u8>)>,
    },
}

/// Configuration non secrète d'une session MQTT. Les éventuels secrets TLS
/// sont injectés uniquement à l'exécution par `MqttTlsConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttBrokerConfiguration {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub tls: Option<MqttTlsConfiguration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboundMqttMessage {
    pub topic: String,
    pub payload: Vec<u8>,
    pub retained: bool,
}

/// Client `rumqttc` isolé du reste de l'adaptateur. Le thread de connexion
/// reste vivant pendant les pertes de réseau ; les messages livrés à Robine
/// passent par une file bornée pour éviter toute croissance mémoire.
pub struct RumqttPublisher {
    client: Mutex<Client>,
}

impl RumqttPublisher {
    pub fn connect(
        configuration: MqttBrokerConfiguration,
        password: Option<String>,
    ) -> Result<(Arc<Self>, mpsc::Receiver<InboundMqttMessage>), MqttError> {
        if configuration.host.trim().is_empty()
            || configuration.client_id.trim().is_empty()
            || configuration.port == 0
        {
            return Err(MqttError::InvalidMessage(
                "MQTT broker configuration is incomplete".into(),
            ));
        }
        let mut options = MqttOptions::new(
            configuration.client_id,
            configuration.host,
            configuration.port,
        );
        options.set_keep_alive(Duration::from_secs(15));
        apply_tls(&mut options, configuration.tls);
        if let Some(username) = configuration.username {
            options.set_credentials(username, password.unwrap_or_default());
        }
        let (client, mut connection) = Client::new(options, 256);
        client
            .subscribe("robine/v1/discovery/+/config", QoS::AtLeastOnce)
            .map_err(|error| MqttError::Transport(error.to_string()))?;
        client
            .subscribe("homeassistant/#", QoS::AtLeastOnce)
            .map_err(|error| MqttError::Transport(error.to_string()))?;
        let publisher = Arc::new(Self {
            client: Mutex::new(client),
        });
        let (sender, receiver) = mpsc::sync_channel(512);
        thread::Builder::new()
            .name("robine-mqtt-connection".into())
            .spawn(move || {
                for notification in connection.iter() {
                    let Ok(Event::Incoming(Packet::Publish(publish))) = notification else {
                        continue;
                    };
                    let _ = sender.try_send(InboundMqttMessage {
                        topic: publish.topic,
                        payload: publish.payload.to_vec(),
                        retained: publish.retain,
                    });
                }
            })
            .map_err(|error| MqttError::Transport(error.to_string()))?;
        Ok((publisher, receiver))
    }
}

fn apply_tls(options: &mut MqttOptions, tls: Option<MqttTlsConfiguration>) {
    if let Some(tls) = tls {
        // `rumqttc` deliberately leaves the Rustls provider unselected. Robine
        // selects `ring` once for the process before constructing its TLS
        // transport; an embedding process may already have selected another
        // provider, which remains authoritative.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let transport = match tls {
            MqttTlsConfiguration::SystemRoots => Transport::tls_with_default_config(),
            MqttTlsConfiguration::Custom {
                ca_certificate_pem,
                client_auth,
            } => Transport::tls(ca_certificate_pem, client_auth, None),
        };
        options.set_transport(transport);
    }
}

impl MqttPublisher for RumqttPublisher {
    fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<(), MqttError> {
        self.client
            .lock()
            .map_err(|_| MqttError::Transport("MQTT publisher lock poisoned".into()))?
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .map_err(|error| MqttError::Transport(error.to_string()))
    }

    fn subscribe(&self, topic: &str) -> Result<(), MqttError> {
        self.client
            .lock()
            .map_err(|_| MqttError::Transport("MQTT publisher lock poisoned".into()))?
            .subscribe(topic, QoS::AtLeastOnce)
            .map_err(|error| MqttError::Transport(error.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum MqttError {
    #[error("MQTT transport failed: {0}")]
    Transport(String),
    #[error("invalid MQTT message: {0}")]
    InvalidMessage(String),
    #[error("unsupported MQTT command")]
    UnsupportedCommand,
}

#[derive(Clone)]
struct ComponentRoute {
    entity_id: EntityId,
    profile: Profile,
    command_topic: Option<String>,
    state_encoding: StateEncoding,
}
#[derive(Clone)]
enum StateEncoding {
    Native,
    HomeAssistant(HaStateEncoding),
}

/// Traduit Robine Discovery et les états JSON vers les ports applicatifs.
/// L'implémentation de connexion (TLS, session, reconnexion) peut alimenter
/// ces méthodes sans exposer son client au cœur.
pub struct MqttAdapter<P: MqttPublisher> {
    publisher: Arc<P>,
    service: HomeService,
    components_by_state_topic: Mutex<HashMap<String, ComponentRoute>>,
    components_by_entity: Mutex<HashMap<EntityId, ComponentRoute>>,
}

impl<P: MqttPublisher> MqttAdapter<P> {
    pub fn new(publisher: Arc<P>, service: HomeService) -> Self {
        Self {
            publisher,
            service,
            components_by_state_topic: Mutex::new(HashMap::new()),
            components_by_entity: Mutex::new(HashMap::new()),
        }
    }

    pub fn ingest_discovery(
        &self,
        topic: &str,
        payload: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Device, ApplicationError> {
        let (config, encodings) = if topic.starts_with("homeassistant/") {
            let imported = robine_mqtt_compat_ha::import(topic, payload)
                .map_err(|error| ApplicationError::Validation(error.to_string()))?;
            let encodings = vec![
                StateEncoding::HomeAssistant(imported.state);
                imported.config.components.len()
            ];
            (imported.config, encodings)
        } else {
            let config = DiscoveryConfig::parse(payload)
                .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
            if topic
                != discovery_topic(&config.origin_id)
                    .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?
            {
                return Err(ApplicationError::Validation(
                    "MQTT discovery topic does not match origin_id".into(),
                ));
            }
            let encodings = vec![StateEncoding::Native; config.components.len()];
            (config, encodings)
        };
        let adapter_id = AdapterId::new(ADAPTER_ID)?;
        let discovery = DeviceDiscovery {
            adapter_id,
            protocol_address: config.origin_id.clone(),
            name: config.device.name,
            entities: config
                .components
                .iter()
                .map(|component| {
                    Ok::<_, ApplicationError>(DiscoveryEntity {
                        protocol_address: component.id.clone(),
                        name: component.name.clone(),
                        kind: profile_kind(&component.profile).into(),
                        capabilities: profile_capabilities(&component.profile)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let device = self.service.register_discovery(discovery, now)?;
        let mut state_routes = self.components_by_state_topic.lock().map_err(|_| {
            ApplicationError::Infrastructure("MQTT state routes lock poisoned".into())
        })?;
        let mut entity_routes = self.components_by_entity.lock().map_err(|_| {
            ApplicationError::Infrastructure("MQTT entity routes lock poisoned".into())
        })?;
        for ((component, encoding), entity) in config
            .components
            .iter()
            .zip(encodings)
            .zip(&device.entities)
        {
            let route = ComponentRoute {
                entity_id: entity.id.clone(),
                profile: component.profile.clone(),
                command_topic: component.command_topic.clone(),
                state_encoding: encoding,
            };
            state_routes.insert(component.state_topic.clone(), route.clone());
            entity_routes.insert(entity.id.clone(), route);
            self.publisher
                .subscribe(&component.state_topic)
                .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        }
        self.service.update_adapter_health(AdapterHealth {
            adapter_id: AdapterId::new(ADAPTER_ID)?,
            status: AdapterStatus::Available,
            detail: None,
            observed_at: now,
        })?;
        Ok(device)
    }

    pub fn ingest_state(
        &self,
        topic: &str,
        payload: &[u8],
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        if payload.len() > MAX_STATE_BYTES {
            return Err(ApplicationError::Validation(
                "MQTT state payload exceeds limit".into(),
            ));
        }
        let route = self
            .components_by_state_topic
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("MQTT state routes lock poisoned".into())
            })?
            .get(topic)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::Validation("MQTT state topic is not registered".into())
            })?;
        let value: Value = serde_json::from_slice(payload).map_err(|_| {
            ApplicationError::Validation("MQTT state payload is not valid JSON".into())
        })?;
        reject_templates(&value)?;
        for (key, state) in canonical_states(&route.profile, &value, &route.state_encoding)? {
            self.service.apply_reported_state(
                ReportedState {
                    entity_id: route.entity_id.clone(),
                    key,
                    value: state,
                    source_at: now,
                },
                now,
            )?;
        }
        Ok(())
    }
}

impl<P: MqttPublisher> CommandDispatcher for MqttAdapter<P> {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let route = self
            .components_by_entity
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("MQTT entity routes lock poisoned".into())
            })?
            .get(&command.entity_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("no MQTT route for entity".into()))?;
        let topic = route.command_topic.ok_or_else(|| {
            ApplicationError::Infrastructure("MQTT component is read-only".into())
        })?;
        let payload = match (command.key.as_str(), command.value, &route.state_encoding) {
            ("switch", StateValue::Bool(on), StateEncoding::Native) => {
                serde_json::to_vec(&serde_json::json!({ "on": on }))
            }
            ("switch", StateValue::Bool(on), StateEncoding::HomeAssistant(encoding)) => Ok(if on {
                encoding.on_payload.as_bytes().to_vec()
            } else {
                encoding.off_payload.as_bytes().to_vec()
            }),
            ("light.brightness", StateValue::Percentage(percent), StateEncoding::Native) => {
                serde_json::to_vec(&serde_json::json!({ "brightness": percent.clamp(0.0, 100.0) }))
            }
            _ => {
                return Err(ApplicationError::Infrastructure(
                    MqttError::UnsupportedCommand.to_string(),
                ));
            }
        }
        .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        self.publisher
            .publish(&topic, payload)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))
    }
}

fn profile_kind(profile: &Profile) -> &'static str {
    match profile {
        Profile::Light => "light",
        Profile::Switch => "switch",
        Profile::Sensor => "sensor",
        Profile::BinarySensor => "binary-sensor",
        Profile::Cover => "cover",
        Profile::Climate => "climate",
    }
}
fn profile_capabilities(profile: &Profile) -> Result<Vec<Capability>, ApplicationError> {
    let keys: &[&str] = match profile {
        Profile::Light => &["switch", "light.brightness"],
        Profile::Switch => &["switch"],
        Profile::Sensor => &["sensor.value"],
        Profile::BinarySensor => &["sensor.binary"],
        Profile::Cover => &["cover.position"],
        Profile::Climate => &["climate.target-temperature"],
    };
    keys.iter()
        .map(|key| Capability::new(*key, 1).map_err(ApplicationError::from))
        .collect()
}
fn canonical_states(
    profile: &Profile,
    value: &Value,
    encoding: &StateEncoding,
) -> Result<Vec<(String, StateValue)>, ApplicationError> {
    if let StateEncoding::HomeAssistant(encoding) = encoding {
        return canonical_ha_states(profile, value, encoding);
    }
    let object = value
        .as_object()
        .ok_or_else(|| ApplicationError::Validation("MQTT state must be a JSON object".into()))?;
    let mut states = Vec::new();
    match profile {
        Profile::Light | Profile::Switch => {
            if let Some(on) = object.get("on").and_then(Value::as_bool) {
                states.push(("switch".into(), StateValue::Bool(on)));
            }
        }
        Profile::BinarySensor => {
            if let Some(value) = object.get("value").and_then(Value::as_bool) {
                states.push(("sensor.binary".into(), StateValue::Bool(value)));
            }
        }
        Profile::Sensor => {
            if let Some(value) = object.get("value") {
                states.push(("sensor.value".into(), StateValue::Text(value.to_string())));
            }
        }
        _ => {}
    }
    if matches!(profile, Profile::Light) {
        if let Some(brightness) = object.get("brightness").and_then(Value::as_f64) {
            states.push((
                "light.brightness".into(),
                StateValue::Percentage(brightness.clamp(0.0, 100.0)),
            ));
        }
    }
    if states.is_empty() {
        return Err(ApplicationError::Validation(
            "MQTT state has no supported property for profile".into(),
        ));
    }
    Ok(states)
}
fn canonical_ha_states(
    profile: &Profile,
    value: &Value,
    encoding: &HaStateEncoding,
) -> Result<Vec<(String, StateValue)>, ApplicationError> {
    let text = value.as_str().ok_or_else(|| {
        ApplicationError::Validation("Home Assistant MQTT state must be a JSON string".into())
    })?;
    match profile {
        Profile::Light | Profile::Switch => {
            let on = if text == encoding.on_payload {
                true
            } else if text == encoding.off_payload {
                false
            } else {
                return Err(ApplicationError::Validation(
                    "Home Assistant MQTT state does not match configured payloads".into(),
                ));
            };
            Ok(vec![("switch".into(), StateValue::Bool(on))])
        }
        Profile::BinarySensor => {
            let on = if text == encoding.on_payload {
                true
            } else if text == encoding.off_payload {
                false
            } else {
                return Err(ApplicationError::Validation(
                    "Home Assistant MQTT state does not match configured payloads".into(),
                ));
            };
            Ok(vec![("sensor.binary".into(), StateValue::Bool(on))])
        }
        Profile::Sensor => Ok(vec![("sensor.value".into(), StateValue::Text(text.into()))]),
        _ => Err(ApplicationError::Validation(
            "Home Assistant MQTT profile state is not supported".into(),
        )),
    }
}
fn reject_templates(value: &Value) -> Result<(), ApplicationError> {
    if contains_template(value) {
        Err(ApplicationError::Validation(
            "MQTT templates are forbidden".into(),
        ))
    } else {
        Ok(())
    }
}
fn contains_template(value: &Value) -> bool {
    match value {
        Value::String(value) => ["{{", "}}", "{%", "%}"]
            .iter()
            .any(|template| value.contains(template)),
        Value::Array(values) => values.iter().any(contains_template),
        Value::Object(values) => values.values().any(contains_template),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::HomeRepository;
    use robine_store_sqlite::SqliteStore;

    #[derive(Default)]
    struct FakePublisher(Mutex<Vec<(String, Vec<u8>)>>);
    impl MqttPublisher for FakePublisher {
        fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<(), MqttError> {
            self.0.lock().unwrap().push((topic.into(), payload));
            Ok(())
        }
    }

    #[test]
    fn discovery_state_and_command_use_only_registered_topics() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let publisher = Arc::new(FakePublisher::default());
        let adapter = Arc::new(MqttAdapter::new(
            publisher.clone(),
            HomeService::new(store.clone(), store.clone(), Arc::new(Noop)),
        ));
        let payload = br#"{"schema_version":1,"origin_id":"lamp.kitchen","device":{"name":"Cuisine","manufacturer":null,"model":null},"components":[{"id":"main","name":"Lampe","profile":"light","state_topic":"zigbee/lamp/state","command_topic":"zigbee/lamp/set","availability_topic":null}]}"#;
        let device = adapter
            .ingest_discovery(
                "robine/v1/discovery/lamp.kitchen/config",
                payload,
                Utc::now(),
            )
            .unwrap();
        adapter
            .ingest_state(
                "zigbee/lamp/state",
                br#"{"on":true,"brightness":42}"#,
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        adapter
            .dispatch(Command {
                id: CommandId::new(),
                entity_id: entity.clone(),
                key: "switch".into(),
                value: StateValue::Bool(false),
                correlation_id: "cor".into(),
                idempotency_key: "key".into(),
                requested_at: Utc::now(),
                status: CommandStatus::Requested,
            })
            .unwrap();
        assert_eq!(publisher.0.lock().unwrap()[0].0, "zigbee/lamp/set");
        assert_eq!(store.get_entity_state(&entity).unwrap().len(), 2);
    }

    #[test]
    fn imports_home_assistant_light_and_translates_its_on_off_payloads() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let publisher = Arc::new(FakePublisher::default());
        let adapter = Arc::new(MqttAdapter::new(
            publisher.clone(),
            HomeService::new(store.clone(), store.clone(), Arc::new(Noop)),
        ));
        let device = adapter
            .ingest_discovery(
                "homeassistant/light/kitchen/lamp/config",
                br#"{"name":"Lampe","unique_id":"lamp.kitchen","state_topic":"zigbee/lamp/state","command_topic":"zigbee/lamp/set","payload_on":"ON","payload_off":"OFF"}"#,
                Utc::now(),
            )
            .unwrap();
        adapter
            .ingest_state("zigbee/lamp/state", br#""ON""#, Utc::now())
            .unwrap();
        let entity = device.entities[0].id.clone();
        adapter
            .dispatch(Command {
                id: CommandId::new(),
                entity_id: entity.clone(),
                key: "switch".into(),
                value: StateValue::Bool(false),
                correlation_id: "cor".into(),
                idempotency_key: "key".into(),
                requested_at: Utc::now(),
                status: CommandStatus::Requested,
            })
            .unwrap();
        assert_eq!(
            publisher.0.lock().unwrap()[0],
            ("zigbee/lamp/set".into(), b"OFF".to_vec())
        );
        assert_eq!(
            store.get_entity_state(&entity).unwrap()[0].value,
            StateValue::Bool(true)
        );
    }

    #[test]
    fn nested_json_state_is_not_mistaken_for_a_template() {
        assert!(reject_templates(&serde_json::json!({ "nested": { "value": true } })).is_ok());
        assert!(reject_templates(&serde_json::json!({ "value": "{{ forbidden }}" })).is_err());
    }

    #[test]
    fn tls_configuration_never_falls_back_to_plain_tcp() {
        let mut options = MqttOptions::new("robine-test", "broker.local", 8883);
        apply_tls(&mut options, Some(MqttTlsConfiguration::SystemRoots));
        assert!(matches!(options.transport(), Transport::Tls(_)));
    }

    struct Noop;
    impl CommandDispatcher for Noop {
        fn dispatch(&self, _: Command) -> Result<(), ApplicationError> {
            Ok(())
        }
    }
}
