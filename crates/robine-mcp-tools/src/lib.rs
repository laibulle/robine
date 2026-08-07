//! Adaptation MCP -> cas d'utilisation, sans HTTP ni accès direct au store.

use chrono::Utc;
use robine_application::{
    ApplicationError, DevicePageRequest, FlowError, FlowService, HomeService,
};
use robine_domain::{DeviceId, EntityId, FlowId, StateValue};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Clone)]
pub struct McpTools {
    service: HomeService,
    flows: FlowService,
}
impl McpTools {
    pub fn new(service: HomeService, flows: FlowService) -> Self {
        Self { service, flows }
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
    pub fn list_devices(
        &self,
        cursor: Option<&str>,
        limit: Option<u64>,
    ) -> Result<Value, ToolError> {
        let limit = limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(ToolError::InvalidArguments(
                "limit must be between 1 and 100".into(),
            ));
        }
        let cursor = cursor
            .map(|cursor| {
                uuid::Uuid::parse_str(cursor)
                    .map(DeviceId)
                    .map_err(|_| ToolError::InvalidArguments("cursor must be a UUID".into()))
            })
            .transpose()?;
        let page = self
            .service
            .list_devices_page(DevicePageRequest {
                cursor,
                limit: limit as usize,
                status: None,
            })
            .map_err(ToolError::Application)?;
        Ok(json!({
            "devices": page.devices,
            "next_cursor": page.next_cursor.map(|cursor| cursor.to_string()),
        }))
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
    pub fn history_query(
        &self,
        entity_id: &str,
        property: Option<&str>,
        after: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Value, ToolError> {
        let entity_id = EntityId(
            uuid::Uuid::parse_str(entity_id)
                .map_err(|_| ToolError::InvalidArguments("entity_id must be a UUID".into()))?,
        );
        let limit = limit.unwrap_or(100);
        if !(1..=100).contains(&limit) {
            return Err(ToolError::InvalidArguments(
                "limit must be between 1 and 100".into(),
            ));
        }
        let mut events = self
            .service
            .events_after(after.unwrap_or(0), limit as usize + 1)
            .map_err(ToolError::Application)?;
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next_after = has_more
            .then(|| events.last().map(|event| event.sequence))
            .flatten();
        let matching = events.into_iter().filter(|event| matches!(&event.data, robine_domain::EventData::StateReported { state } if state.entity_id == entity_id && property.is_none_or(|property| state.key == property))).collect::<Vec<_>>();
        Ok(json!({ "events": matching, "next_after": next_after }))
    }
    pub fn list_automations(&self) -> Result<Value, ToolError> {
        Ok(
            serde_json::to_value(self.flows.list().map_err(ToolError::Flow)?)
                .expect("flow definitions serialize"),
        )
    }
    pub fn simulate_automation(&self, flow_id: &str) -> Result<Value, ToolError> {
        let flow_id = FlowId(
            uuid::Uuid::parse_str(flow_id)
                .map_err(|_| ToolError::InvalidArguments("flow_id must be a UUID".into()))?,
        );
        Ok(serde_json::to_value(
            self.flows
                .simulate_existing(&flow_id)
                .map_err(ToolError::Flow)?,
        )
        .expect("flow simulation serializes"))
    }
    pub fn automation_get(&self, flow_id: &str) -> Result<Value, ToolError> {
        let flow_id = FlowId(
            uuid::Uuid::parse_str(flow_id)
                .map_err(|_| ToolError::InvalidArguments("flow_id must be a UUID".into()))?,
        );
        Ok(
            serde_json::to_value(self.flows.get(&flow_id).map_err(ToolError::Flow)?)
                .expect("flow definition serializes"),
        )
    }
    pub fn automation_explain(&self, run_id: &str) -> Result<Value, ToolError> {
        self.flows.explain_run(run_id).map_err(ToolError::Flow)
    }
    pub fn set_automation_enabled(&self, flow_id: &str, enabled: bool) -> Result<Value, ToolError> {
        let id = FlowId(
            uuid::Uuid::parse_str(flow_id)
                .map_err(|_| ToolError::InvalidArguments("flow_id must be a UUID".into()))?,
        );
        let previous = self.flows.get(&id).map_err(ToolError::Flow)?;
        let flow = self
            .flows
            .update(id, previous.source, enabled, Utc::now())
            .map_err(ToolError::Flow)?;
        Ok(serde_json::to_value(flow).expect("flow definition serializes"))
    }
    pub fn device_get(&self, device_id: &str) -> Result<Value, ToolError> {
        let device_id = uuid::Uuid::parse_str(device_id)
            .map_err(|_| ToolError::InvalidArguments("device_id must be a UUID".into()))?;
        let device = self
            .service
            .list_devices()
            .map_err(ToolError::Application)?
            .into_iter()
            .find(|device| device.id.0 == device_id)
            .ok_or(ToolError::NotFound)?;
        Ok(serde_json::to_value(device).expect("domain device serializes"))
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
    Flow(#[from] FlowError),
    #[error(transparent)]
    Application(#[from] ApplicationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::CommandDispatcher;
    use robine_domain::{AdapterId, Capability, Command, DeviceDiscovery, DiscoveryEntity};
    use robine_store_sqlite::SqliteStore;
    use std::sync::Arc;

    struct Noop;
    impl CommandDispatcher for Noop {
        fn dispatch(&self, _: Command) -> Result<(), ApplicationError> {
            Ok(())
        }
    }

    #[test]
    fn device_listing_uses_a_bounded_cursor_page() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(store.clone(), store.clone(), Arc::new(Noop));
        for name in ["Aube", "Brume", "Cèdre"] {
            home.register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:mcp").unwrap(),
                    protocol_address: name.into(),
                    name: name.into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: format!("{name}:light"),
                        name: name.into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        }
        let tools = McpTools::new(home, FlowService::new(store.clone(), store));
        let first = tools.list_devices(None, Some(2)).unwrap();
        assert_eq!(first["devices"].as_array().unwrap().len(), 2);
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = tools.list_devices(Some(cursor), Some(2)).unwrap();
        assert_eq!(second["devices"].as_array().unwrap().len(), 1);
        assert!(second["next_cursor"].is_null());
    }
}
