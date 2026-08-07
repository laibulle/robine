//! Contrat RPC local versionné entre Robine et `robine-matterd`.
//! Aucun secret de fabric ne figure dans ces messages.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const RPC_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcRequest<T> {
    pub rpc_version: u16,
    pub request_id: String,
    pub body: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcResponse<T> {
    pub rpc_version: u16,
    pub request_id: String,
    pub body: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterRequest {
    Health,
    StartCommissioning {
        setup_code: String,
    },
    GetJob {
        job_id: String,
    },
    ListEndpoints,
    Invoke {
        fabric_id: String,
        node_id: String,
        endpoint_id: u16,
        command: ClusterCommand,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterResponse {
    Health {
        available: bool,
        detail: Option<String>,
    },
    CommissioningStarted {
        job_id: String,
    },
    Job {
        job: CommissioningJob,
    },
    Endpoints {
        endpoints: Vec<Endpoint>,
    },
    InvocationAccepted {
        invocation_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommissioningJob {
    pub id: String,
    pub status: JobStatus,
    pub progress: u8,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    InProgress,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub fabric_id: String,
    pub node_id: String,
    pub endpoint_id: u16,
    pub name: String,
    pub clusters: Vec<Cluster>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cluster {
    OnOff,
    LevelControl,
    ColorControl,
    TemperatureMeasurement,
    RelativeHumidityMeasurement,
    OccupancySensing,
    BooleanState,
    ElectricalPowerMeasurement,
    ElectricalEnergyMeasurement,
    Thermostat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClusterCommand {
    SetOnOff { on: bool },
    SetLevel { percent: u8 },
    SetTemperature { centi_celsius: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatterEvent {
    AttributeReported {
        fabric_id: String,
        node_id: String,
        endpoint_id: u16,
        attribute: AttributeValue,
    },
    AvailabilityChanged {
        fabric_id: String,
        node_id: String,
        available: bool,
    },
    JobProgress {
        job: CommissioningJob,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttributeValue {
    OnOff { on: bool },
    Level { percent: u8 },
    Temperature { centi_celsius: i32 },
    Humidity { centi_percent: u16 },
    Occupancy { occupied: bool },
}

impl<T> RpcRequest<T> {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.rpc_version != RPC_VERSION {
            return Err(ContractError::UnsupportedVersion(self.rpc_version));
        }
        validate_id("request_id", &self.request_id)
    }
}
impl CommissioningJob {
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_id("job_id", &self.id)?;
        if self.progress > 100 {
            return Err(ContractError::InvalidProgress);
        }
        Ok(())
    }
}
fn validate_id(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > 160 || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ContractError::InvalidId(field));
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("unsupported Matter RPC version {0}")]
    UnsupportedVersion(u16),
    #[error("invalid {0}")]
    InvalidId(&'static str),
    #[error("job progress must be between 0 and 100")]
    InvalidProgress,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rpc_request_is_versioned_and_contains_no_fabric_secret() {
        let request = RpcRequest {
            rpc_version: RPC_VERSION,
            request_id: "req-1".into(),
            body: MatterRequest::StartCommissioning {
                setup_code: "34970112332".into(),
            },
        };
        request.validate().unwrap();
        let serialized = serde_json::to_string(&request).unwrap();
        assert!(!serialized.contains("private_key"));
    }
}
