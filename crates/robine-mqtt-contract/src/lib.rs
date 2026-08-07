//! Contrat JSON borné de Robine Discovery sur MQTT, sans client de broker.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DISCOVERY_SCHEMA_VERSION: u16 = 1;
pub const MAX_DISCOVERY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    pub schema_version: u16,
    pub origin_id: String,
    pub device: DeviceDescriptor,
    pub components: Vec<ComponentDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    pub id: String,
    pub name: String,
    pub profile: Profile,
    pub state_topic: String,
    pub command_topic: Option<String>,
    pub availability_topic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Light,
    Switch,
    Sensor,
    BinarySensor,
    Cover,
    Climate,
}

impl DiscoveryConfig {
    pub fn parse(payload: &[u8]) -> Result<Self, ContractError> {
        if payload.len() > MAX_DISCOVERY_BYTES {
            return Err(ContractError::PayloadTooLarge);
        }
        let text = std::str::from_utf8(payload).map_err(|_| ContractError::InvalidUtf8)?;
        reject_templates(text)?;
        let value: Self = serde_json::from_str(text)
            .map_err(|error| ContractError::InvalidJson(error.to_string()))?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != DISCOVERY_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema(self.schema_version));
        }
        validate_identifier("origin_id", &self.origin_id)?;
        if self.device.name.trim().is_empty() {
            return Err(ContractError::MissingField("device.name"));
        }
        if self.components.is_empty() {
            return Err(ContractError::MissingField("components"));
        }
        let mut component_ids = std::collections::HashSet::new();
        for component in &self.components {
            validate_identifier("component.id", &component.id)?;
            if !component_ids.insert(&component.id) {
                return Err(ContractError::DuplicateComponent(component.id.clone()));
            }
            if component.name.trim().is_empty() {
                return Err(ContractError::MissingField("component.name"));
            }
            validate_topic(&component.state_topic)?;
            if let Some(topic) = &component.command_topic {
                validate_topic(topic)?;
            }
            if let Some(topic) = &component.availability_topic {
                validate_topic(topic)?;
            }
            if matches!(
                component.profile,
                Profile::Light | Profile::Switch | Profile::Cover | Profile::Climate
            ) && component.command_topic.is_none()
            {
                return Err(ContractError::CommandTopicRequired(component.id.clone()));
            }
        }
        Ok(())
    }
}

pub fn discovery_topic(origin_id: &str) -> Result<String, ContractError> {
    validate_identifier("origin_id", origin_id)?;
    Ok(format!("robine/v1/discovery/{origin_id}/config"))
}
pub fn state_topic(origin_id: &str) -> Result<String, ContractError> {
    validate_identifier("origin_id", origin_id)?;
    Ok(format!("robine/v1/state/{origin_id}"))
}
pub fn availability_topic(origin_id: &str) -> Result<String, ContractError> {
    validate_identifier("origin_id", origin_id)?;
    Ok(format!("robine/v1/availability/{origin_id}"))
}
pub fn command_topic(origin_id: &str) -> Result<String, ContractError> {
    validate_identifier("origin_id", origin_id)?;
    Ok(format!("robine/v1/command/{origin_id}"))
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ContractError::InvalidIdentifier(field));
    }
    Ok(())
}
fn validate_topic(topic: &str) -> Result<(), ContractError> {
    if topic.is_empty()
        || topic.len() > 512
        || topic.contains(['#', '+', '\0'])
        || topic.starts_with('/')
        || topic.ends_with('/')
    {
        return Err(ContractError::InvalidTopic(topic.into()));
    }
    Ok(())
}
fn reject_templates(value: &str) -> Result<(), ContractError> {
    if value.contains("{{") || value.contains("}}") || value.contains("{%") || value.contains("%}")
    {
        return Err(ContractError::TemplateForbidden);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("MQTT payload exceeds the configured discovery limit")]
    PayloadTooLarge,
    #[error("MQTT payload is not UTF-8")]
    InvalidUtf8,
    #[error("MQTT payload is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported discovery schema version {0}")]
    UnsupportedSchema(u16),
    #[error("invalid {0}")]
    InvalidIdentifier(&'static str),
    #[error("missing required field {0}")]
    MissingField(&'static str),
    #[error("duplicate component {0}")]
    DuplicateComponent(String),
    #[error("invalid MQTT topic {0}")]
    InvalidTopic(String),
    #[error("component {0} requires a command topic")]
    CommandTopicRequired(String),
    #[error("templates and executable expressions are forbidden")]
    TemplateForbidden,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_a_bounded_native_discovery_document() {
        let config = DiscoveryConfig::parse(br#"{"schema_version":1,"origin_id":"lamp.kitchen","device":{"name":"Cuisine","manufacturer":null,"model":null},"components":[{"id":"main","name":"Lampe","profile":"light","state_topic":"zigbee/lamp/state","command_topic":"zigbee/lamp/set","availability_topic":null}]}"#).unwrap();
        assert_eq!(
            discovery_topic(&config.origin_id).unwrap(),
            "robine/v1/discovery/lamp.kitchen/config"
        );
    }
    #[test]
    fn rejects_templates_before_discovery() {
        let error = DiscoveryConfig::parse(br#"{"value_template":"{{ states('sensor.x') }}"}"#)
            .unwrap_err();
        assert_eq!(error, ContractError::TemplateForbidden);
    }
}
