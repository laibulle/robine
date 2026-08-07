//! Adaptateur MQTT borné. Les topics et payloads ne traversent jamais cette
//! frontière vers le domaine ni les cas d'utilisation.

use chrono::{DateTime, Utc};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::*;
use robine_mqtt_contract::{DiscoveryConfig, Profile, discovery_topic};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub const ADAPTER_ID: &str = "mqtt:local";
pub const MAX_STATE_BYTES: usize = 16 * 1024;

pub trait MqttPublisher: Send + Sync {
    fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<(), MqttError>;
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
        for (component, entity) in config.components.iter().zip(&device.entities) {
            let route = ComponentRoute {
                entity_id: entity.id.clone(),
                profile: component.profile.clone(),
                command_topic: component.command_topic.clone(),
            };
            state_routes.insert(component.state_topic.clone(), route.clone());
            entity_routes.insert(entity.id.clone(), route);
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
        for (key, state) in canonical_states(&route.profile, &value)? {
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
        let payload = match (command.key.as_str(), command.value) {
            ("switch", StateValue::Bool(on)) => {
                serde_json::to_vec(&serde_json::json!({ "on": on }))
            }
            ("light.brightness", StateValue::Percentage(percent)) => {
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
) -> Result<Vec<(String, StateValue)>, ApplicationError> {
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
fn reject_templates(value: &Value) -> Result<(), ApplicationError> {
    let source = value.to_string();
    if ["{{", "}}", "{%", "%}"]
        .iter()
        .any(|template| source.contains(template))
    {
        Err(ApplicationError::Validation(
            "MQTT templates are forbidden".into(),
        ))
    } else {
        Ok(())
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
    struct Noop;
    impl CommandDispatcher for Noop {
        fn dispatch(&self, _: Command) -> Result<(), ApplicationError> {
            Ok(())
        }
    }
}
