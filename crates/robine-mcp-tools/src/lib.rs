//! Adaptation MCP -> cas d'utilisation, sans HTTP ni accès direct au store.

use chrono::Utc;
use robine_application::{ApplicationError, HomeService};
use robine_domain::{EntityId, StateValue};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Clone)]
pub struct McpTools {
    service: HomeService,
}
impl McpTools {
    pub fn new(service: HomeService) -> Self {
        Self { service }
    }
    pub fn home_summary(&self) -> Result<Value, ToolError> {
        let devices = self
            .service
            .list_devices()
            .map_err(ToolError::Application)?;
        let unavailable = devices
            .iter()
            .filter(|device| matches!(device.status, robine_domain::DeviceStatus::Unavailable))
            .count();
        Ok(
            json!({ "devices": devices.len(), "unavailable_devices": unavailable, "status": if unavailable == 0 { "healthy" } else { "degraded" } }),
        )
    }
    pub fn list_devices(&self) -> Result<Value, ToolError> {
        Ok(serde_json::to_value(
            self.service
                .list_devices()
                .map_err(ToolError::Application)?,
        )
        .expect("domain devices serialize"))
    }
    pub fn entity_get(&self, entity_id: &str) -> Result<Value, ToolError> {
        let entity_id = EntityId(
            uuid::Uuid::parse_str(entity_id)
                .map_err(|_| ToolError::InvalidArguments("entity_id must be a UUID".into()))?,
        );
        let detail = self
            .service
            .entity_detail(&entity_id)
            .map_err(ToolError::Application)?
            .ok_or(ToolError::NotFound)?;
        Ok(serde_json::to_value(detail).expect("entity detail serializes"))
    }
    pub fn history_query(&self, entity_id: &str) -> Result<Value, ToolError> {
        let entity_id = EntityId(
            uuid::Uuid::parse_str(entity_id)
                .map_err(|_| ToolError::InvalidArguments("entity_id must be a UUID".into()))?,
        );
        let events = self
            .service
            .events_after(0, 500)
            .map_err(ToolError::Application)?;
        Ok(serde_json::to_value(events.into_iter().filter(|event| matches!(&event.data, robine_domain::EventData::StateReported { state } if state.entity_id == entity_id)).collect::<Vec<_>>()).expect("events serialize"))
    }
    pub fn request_command(
        &self,
        entity_id: &str,
        key: String,
        value: StateValue,
        approval_id: &str,
    ) -> Result<Value, ToolError> {
        if approval_id.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "approval_id is required".into(),
            ));
        }
        let entity_id = EntityId(
            uuid::Uuid::parse_str(entity_id)
                .map_err(|_| ToolError::InvalidArguments("entity_id must be a UUID".into()))?,
        );
        let command = self
            .service
            .request_command(
                entity_id,
                key,
                value,
                format!("mcp:{approval_id}"),
                Utc::now(),
            )
            .map_err(ToolError::Application)?;
        Ok(json!({ "command_id": command.id, "correlation_id": command.correlation_id }))
    }
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
    #[error("requested resource was not found")]
    NotFound,
    #[error(transparent)]
    Application(#[from] ApplicationError),
}
