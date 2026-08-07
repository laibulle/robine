//! Le modèle canonique de Robine. Ce crate ne connaît ni HTTP, ni SQLite, ni Hue.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

opaque_id!(DeviceId);
opaque_id!(EntityId);
opaque_id!(AreaId);
opaque_id!(CommandId);
opaque_id!(FlowId);
opaque_id!(FlowRunId);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdapterId(pub String);

impl AdapterId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::InvalidAdapterId);
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Discovered,
    Available,
    Unavailable,
    Removed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    Starting,
    Available,
    Degraded,
    Unauthorized,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterHealth {
    pub adapter_id: AdapterId,
    pub status: AdapterStatus,
    pub detail: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub key: String,
    pub version: u16,
}

impl Capability {
    pub fn new(key: impl Into<String>, version: u16) -> Result<Self, DomainError> {
        let key = key.into();
        if key.trim().is_empty() || version == 0 {
            return Err(DomainError::InvalidCapability);
        }
        Ok(Self { key, version })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub kind: String,
    pub capabilities: Vec<Capability>,
    pub area_id: Option<AreaId>,
}

impl Entity {
    pub fn supports(&self, key: &str) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.key == key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    pub adapter_id: AdapterId,
    pub protocol_address: String,
    pub name: String,
    pub status: DeviceStatus,
    pub entities: Vec<Entity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Area {
    pub id: AreaId,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FlowDefinition {
    pub id: FlowId,
    pub name: String,
    pub enabled: bool,
    pub revision: u64,
    pub ast: serde_json::Value,
    pub source: String,
    pub source_hash: String,
}

/// Point de reprise durable d'une exécution Flow. Le plan est une donnée
/// compilée et versionnée ; aucune source libre n'est réinterprétée au réveil.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FlowRun {
    pub id: FlowRunId,
    pub flow_id: FlowId,
    pub plan: serde_json::Value,
    pub next_action: usize,
    pub wake_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum StateValue {
    Bool(bool),
    Percentage(f64),
    Text(String),
}

impl StateValue {
    pub fn is_valid_for(&self, key: &str) -> bool {
        matches!(
            (key, self),
            ("switch", Self::Bool(_)) | ("light.brightness", Self::Percentage(_))
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateQuality {
    Reported,
    Estimated,
    Unavailable,
    Invalid,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StateProperty {
    pub entity_id: EntityId,
    pub key: String,
    pub value: StateValue,
    pub quality: StateQuality,
    pub source_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReportedState {
    pub entity_id: EntityId,
    pub key: String,
    pub value: StateValue,
    pub source_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub entity_id: EntityId,
    pub key: String,
    pub value: StateValue,
    pub correlation_id: String,
    pub idempotency_key: String,
    pub requested_at: DateTime<Utc>,
    pub status: CommandStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Requested,
    Dispatched,
    Confirmed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum EventData {
    DeviceRegistered { device: Device },
    DeviceUpdated { device: Device },
    AreaCreated { area: Area },
    AdapterHealthChanged { health: AdapterHealth },
    FlowCreated { flow: FlowDefinition },
    FlowUpdated { flow: FlowDefinition },
    StateReported { state: StateProperty },
    CommandRequested { command: Command },
    CommandDispatched { command: Command },
    CommandConfirmed { command: Command },
    CommandFailed { command: Command, reason: String },
}

impl EventData {
    pub fn topic(&self) -> &'static str {
        match self {
            Self::DeviceRegistered { .. } | Self::DeviceUpdated { .. } => "device",
            Self::AreaCreated { .. } => "area",
            Self::AdapterHealthChanged { .. } => "adapter",
            Self::FlowCreated { .. } | Self::FlowUpdated { .. } => "automation",
            Self::StateReported { .. } => "state",
            Self::CommandRequested { .. } => "command",
            Self::CommandDispatched { .. }
            | Self::CommandConfirmed { .. }
            | Self::CommandFailed { .. } => "command",
        }
    }
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::DeviceRegistered { .. } => "device.registered",
            Self::DeviceUpdated { .. } => "device.updated",
            Self::AreaCreated { .. } => "area.created",
            Self::AdapterHealthChanged { .. } => "adapter.health_changed",
            Self::FlowCreated { .. } => "automation.created",
            Self::FlowUpdated { .. } => "automation.updated",
            Self::StateReported { .. } => "state.reported",
            Self::CommandRequested { .. } => "command.requested",
            Self::CommandDispatched { .. } => "command.dispatched",
            Self::CommandConfirmed { .. } => "command.confirmed",
            Self::CommandFailed { .. } => "command.failed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Option<String>,
    pub data: EventData,
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("adapter identifier is invalid")]
    InvalidAdapterId,
    #[error("capability is invalid")]
    InvalidCapability,
    #[error("a device must have a protocol address")]
    MissingProtocolAddress,
    #[error("an entity cannot declare the same capability twice")]
    DuplicateCapability,
    #[error("state value does not match capability {0}")]
    InvalidStateValue(String),
}

#[derive(Clone, Debug)]
pub struct DiscoveryEntity {
    pub protocol_address: String,
    pub name: String,
    pub kind: String,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug)]
pub struct DeviceDiscovery {
    pub adapter_id: AdapterId,
    pub protocol_address: String,
    pub name: String,
    pub entities: Vec<DiscoveryEntity>,
}

impl DeviceDiscovery {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.protocol_address.trim().is_empty() {
            return Err(DomainError::MissingProtocolAddress);
        }
        for entity in &self.entities {
            let mut keys = std::collections::HashSet::new();
            for capability in &entity.capabilities {
                if !keys.insert((&capability.key, capability.version)) {
                    return Err(DomainError::DuplicateCapability);
                }
            }
        }
        Ok(())
    }
}
