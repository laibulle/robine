//! Import strict d'un sous-ensemble Home Assistant MQTT Discovery.
//!
//! Aucune expression Jinja, script, template ou commande shell n'est admis.
//! Le résultat est un descripteur Robine Discovery, jamais un modèle HA.

use robine_mqtt_contract::{ComponentDescriptor, DeviceDescriptor, DiscoveryConfig, Profile};
use serde::Deserialize;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedDiscovery {
    pub config: DiscoveryConfig,
    pub state: HaStateEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HaStateEncoding {
    pub on_payload: String,
    pub off_payload: String,
}

#[derive(Deserialize)]
struct HaConfig {
    name: Option<String>,
    unique_id: Option<String>,
    state_topic: String,
    command_topic: Option<String>,
    availability_topic: Option<String>,
    payload_on: Option<String>,
    payload_off: Option<String>,
    device: Option<HaDevice>,
    #[serde(default)]
    value_template: Option<String>,
    #[serde(default)]
    command_template: Option<String>,
}
#[derive(Deserialize)]
struct HaDevice {
    name: Option<String>,
    manufacturer: Option<String>,
    model: Option<String>,
}

pub fn import(topic: &str, payload: &[u8]) -> Result<ImportedDiscovery, HaImportError> {
    let (component, node_id, object_id) = parse_topic(topic)?;
    let text = std::str::from_utf8(payload).map_err(|_| HaImportError::InvalidUtf8)?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| HaImportError::InvalidJson(error.to_string()))?;
    if contains_template(&value) {
        return Err(HaImportError::TemplateForbidden);
    }
    let config: HaConfig = serde_json::from_value(value)
        .map_err(|error| HaImportError::InvalidJson(error.to_string()))?;
    if config.value_template.is_some() || config.command_template.is_some() {
        return Err(HaImportError::TemplateForbidden);
    }
    let profile = match component {
        "light" => Profile::Light,
        "switch" => Profile::Switch,
        "sensor" => Profile::Sensor,
        "binary_sensor" => Profile::BinarySensor,
        "cover" => Profile::Cover,
        "climate" => Profile::Climate,
        _ => return Err(HaImportError::UnsupportedComponent(component.into())),
    };
    let source_id = config
        .unique_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{node_id}.{object_id}"));
    let origin_id = format!("ha.{source_id}");
    let name = config
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| object_id.replace('_', " "));
    let device = config.device.unwrap_or(HaDevice {
        name: Some(node_id.replace('_', " ")),
        manufacturer: None,
        model: None,
    });
    let imported = ImportedDiscovery {
        config: DiscoveryConfig {
            schema_version: 1,
            origin_id,
            device: DeviceDescriptor {
                name: device.name.unwrap_or_else(|| node_id.replace('_', " ")),
                manufacturer: device.manufacturer,
                model: device.model,
            },
            components: vec![ComponentDescriptor {
                id: "main".into(),
                name,
                profile,
                state_topic: config.state_topic,
                command_topic: config.command_topic,
                availability_topic: config.availability_topic,
            }],
        },
        state: HaStateEncoding {
            on_payload: config.payload_on.unwrap_or_else(|| "ON".into()),
            off_payload: config.payload_off.unwrap_or_else(|| "OFF".into()),
        },
    };
    imported
        .config
        .validate()
        .map_err(|error| HaImportError::InvalidDiscovery(error.to_string()))?;
    Ok(imported)
}

fn parse_topic(topic: &str) -> Result<(&str, &str, &str), HaImportError> {
    let parts = topic.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["homeassistant", component, node_id, object_id, "config"]
            if !component.is_empty() && !node_id.is_empty() && !object_id.is_empty() =>
        {
            Ok((component, node_id, object_id))
        }
        _ => Err(HaImportError::InvalidTopic),
    }
}

fn contains_template(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => ["{{", "}}", "{%", "%}"]
            .iter()
            .any(|marker| value.contains(marker)),
        serde_json::Value::Array(values) => values.iter().any(contains_template),
        serde_json::Value::Object(values) => values.values().any(contains_template),
        _ => false,
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HaImportError {
    #[error("Home Assistant discovery topic is invalid")]
    InvalidTopic,
    #[error("Home Assistant discovery payload is not UTF-8")]
    InvalidUtf8,
    #[error("Home Assistant discovery payload is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("Home Assistant templates and expressions are forbidden")]
    TemplateForbidden,
    #[error("unsupported Home Assistant MQTT component {0}")]
    UnsupportedComponent(String),
    #[error("Home Assistant discovery cannot become a valid Robine descriptor: {0}")]
    InvalidDiscovery(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_light_without_executing_a_template() {
        let imported = import(
            "homeassistant/light/kitchen/lamp/config",
            br#"{"name":"Lampe cuisine","unique_id":"lamp.kitchen","state_topic":"zigbee/lamp/state","command_topic":"zigbee/lamp/set","payload_on":"ON","payload_off":"OFF","device":{"name":"Cuisine","manufacturer":"Acme","model":"L1"}}"#,
        )
        .unwrap();
        assert_eq!(imported.config.origin_id, "ha.lamp.kitchen");
        assert_eq!(imported.config.components[0].profile, Profile::Light);
        assert_eq!(imported.state.on_payload, "ON");
    }

    #[test]
    fn refuses_jinja_before_any_descriptor_is_created() {
        assert_eq!(
            import(
                "homeassistant/switch/kitchen/fan/config",
                br#"{"state_topic":"zigbee/fan","command_topic":"zigbee/fan/set","value_template":"{{ value_json.on }}"}"#,
            )
            .unwrap_err(),
            HaImportError::TemplateForbidden
        );
    }
}
