#![recursion_limit = "256"]

//! DTO versionnés, partagés par les transports HTTP et WebSocket.

use robine_domain::{CommandId, Device, EventEnvelope, StateValue};
use robine_matter_contract::CommissioningJob;
use robine_mcp_types::McpWritePolicy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const API_VERSION: &str = "v1";

/// Artefact OpenAPI versionné servant aux clients Swift et aux consoles locales.
pub fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "Robine local API", "version": API_VERSION },
        "paths": {
            "/health": { "get": { "responses": { "200": { "description": "Process health" } } } },
            "/api/v1/devices": { "get": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "cursor", "in": "query", "schema": { "type": "string", "format": "uuid" } }, { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100, "default": 50 } }, { "name": "status", "in": "query", "schema": { "type": "string", "enum": ["discovered", "available", "unavailable", "removed"] } }], "responses": { "200": { "description": "Bounded device page" }, "400": { "description": "Invalid page query" }, "401": { "description": "Authentication required" } } } },
            "/api/v1/devices/{id}": { "patch": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Device renamed" } } }, "delete": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Device logically removed" } } } },
            "/api/v1/entities/{id}": { "get": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }], "responses": { "200": { "description": "Entity detail" }, "404": { "description": "Entity not found" } } } },
            "/api/v1/entities/{id}/area": { "put": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }], "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object", "properties": { "area_id": { "type": ["string", "null"], "format": "uuid" } }, "required": ["area_id"], "additionalProperties": false } } } }, "responses": { "200": { "description": "Entity area updated" }, "404": { "description": "Entity or area not found" } } } },
            "/api/v1/entities/{id}/commands": { "post": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }, { "name": "Idempotency-Key", "in": "header", "required": true, "schema": { "type": "string", "minLength": 1 } }], "requestBody": { "required": true, "content": { "application/json": { "schema": command_request_schema() } } }, "responses": { "202": { "description": "Command accepted" } } } },
            "/api/v1/events": { "get": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "after", "in": "query", "schema": { "type": "integer", "minimum": 0 } }, { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 500 } }, { "name": "tail", "in": "query", "description": "Most recent events; mutually exclusive with after", "schema": { "type": "integer", "minimum": 1, "maximum": 500 } }], "responses": { "200": { "description": "Event replay or recent-event page" } } } },
            "/api/v1/adapters/hue/pair": { "post": { "security": [{ "bearerAuth": [] }], "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object", "required": ["authority", "certificate_pem", "certificate_sha256"], "properties": { "authority": { "type": "string" }, "certificate_pem": { "type": "string", "writeOnly": true }, "certificate_sha256": { "type": "string", "pattern": "^[a-f0-9]{64}$" } }, "additionalProperties": false } } } }, "responses": { "201": { "description": "Bridge paired and synchronized" }, "409": { "description": "Bridge button was not pressed" } } } },
            "/api/v1/adapters/hue/discover": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Locally discovered Hue bridges" } } } },
            "/api/v1/adapters/matter/commission": { "post": { "security": [{ "bearerAuth": [] }], "responses": { "202": { "description": "Matter commissioning job accepted" }, "503": { "description": "Matter controller unavailable" } } } },
            "/api/v1/adapters/matter/jobs/{id}": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Matter commissioning job" } } } },
            "/api/v1/backups": { "post": { "security": [{ "bearerAuth": [] }], "responses": { "201": { "description": "Verified SQLite snapshot" } } } },
            "/api/v1/auth/mcp-tokens": { "post": { "security": [{ "bearerAuth": [] }], "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object", "properties": { "scopes": { "type": "array", "items": { "type": "string" } }, "expires_in_seconds": { "type": "integer", "minimum": 60, "maximum": 2592000 }, "write_policy": { "oneOf": [ { "type": "object", "properties": { "mode": { "const": "read_only" } }, "required": ["mode"], "additionalProperties": false }, { "type": "object", "properties": { "mode": { "const": "confirm_each" } }, "required": ["mode"], "additionalProperties": false }, { "type": "object", "properties": { "mode": { "const": "allow_listed" }, "max_commands_per_hour": { "type": "integer", "minimum": 1, "maximum": 3600 }, "commands": { "type": "array", "minItems": 1, "items": { "type": "object", "properties": { "entity_id": { "type": "string", "format": "uuid" }, "keys": { "type": "array", "minItems": 1, "items": { "type": "string", "minLength": 1 } } }, "required": ["entity_id", "keys"], "additionalProperties": false } } }, "required": ["mode", "max_commands_per_hour", "commands"], "additionalProperties": false } ] } }, "additionalProperties": false } } } }, "responses": { "201": { "description": "Dedicated MCP token" }, "400": { "description": "Invalid scopes or write policy" } } } },
            "/api/v1/auth/mcp-approvals": { "post": { "security": [{ "bearerAuth": [] }], "responses": { "201": { "description": "One-time MCP approval" } } } },
            "/api/v1/stream": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "101": { "description": "Authenticated WebSocket upgrade" } } } }
        },
        "components": { "securitySchemes": { "bearerAuth": { "type": "http", "scheme": "bearer" } }, "schemas": { "CommandRequest": command_request_schema(), "WebSocketClientMessage": websocket_client_schema(), "WebSocketServerMessage": websocket_server_schema() } }
    })
}

pub fn command_request_schema() -> Value {
    json!({ "type": "object", "required": ["key", "value"], "properties": { "key": { "type": "string", "minLength": 1 }, "value": {} }, "additionalProperties": false })
}
pub fn websocket_client_schema() -> Value {
    json!({ "oneOf": [ { "type": "object", "properties": { "type": { "const": "subscribe" }, "topics": { "type": "array", "items": { "enum": ["state", "device", "area", "automation", "adapter", "command"] } }, "after": { "type": "integer", "minimum": 0 } }, "required": ["type", "topics"], "additionalProperties": false }, { "type": "object", "properties": { "type": { "const": "ack" }, "id": { "type": "integer", "minimum": 1 } }, "required": ["type", "id"], "additionalProperties": false } ] })
}
pub fn websocket_server_schema() -> Value {
    json!({ "oneOf": [ { "type": "object", "properties": { "type": { "const": "ready" }, "cursor": { "type": "integer", "minimum": 0 } }, "required": ["type", "cursor"] }, { "type": "object", "properties": { "type": { "const": "event" }, "id": { "type": "integer", "minimum": 1 }, "topic": { "type": "string" }, "event_type": { "type": "string" }, "occurred_at": { "type": "string", "format": "date-time" }, "data": {} }, "required": ["type", "id", "topic", "event_type", "occurred_at", "data"] }, { "type": "object", "properties": { "type": { "const": "resync_required" } }, "required": ["type"] } ] })
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    pub correlation_id: String,
}

#[derive(Debug, Deserialize)]
pub struct BootstrapAdministratorRequest {
    pub password: String,
}
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
}
#[derive(Debug, Deserialize)]
pub struct IssueTokenRequest {
    pub password: String,
}
#[derive(Debug, Deserialize)]
pub struct IssueMcpTokenRequest {
    pub expires_in_seconds: Option<u64>,
    pub scopes: Option<Vec<String>>,
    pub write_policy: Option<McpWritePolicy>,
}
#[derive(Debug, Serialize)]
pub struct McpTokenResponse {
    pub token: String,
    pub token_id: String,
    pub expires_at: String,
    pub scopes: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct CreateMcpApprovalRequest {
    pub token_id: String,
    pub tool: String,
    pub arguments: Value,
    pub expires_in_seconds: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct McpApprovalResponse {
    pub approval_id: String,
    pub expires_at: String,
}
#[derive(Debug, Deserialize)]
pub struct CreateAreaRequest {
    pub name: String,
}
#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub name: String,
}
#[derive(Debug, Deserialize)]
pub struct AssignEntityAreaRequest {
    pub area_id: Option<uuid::Uuid>,
}
#[derive(Debug, Deserialize)]
pub struct FlowUpsertRequest {
    pub source: String,
    pub enabled: bool,
}
#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub key: String,
    pub value: StateValue,
}
#[derive(Debug, Serialize)]
pub struct CommandAccepted {
    pub command_id: CommandId,
    pub correlation_id: String,
}

/// Page stable dans l'ordre d'affichage courant (`name`, puis identifiant).
/// `next_cursor` est opaque pour le client et n'est présent que lorsqu'une
/// page suivante existe.
#[derive(Debug, Serialize)]
pub struct DevicePage {
    pub devices: Vec<Device>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HuePairRequest {
    pub authority: String,
    pub certificate_pem: String,
    pub certificate_sha256: String,
}

/// La clé Hue ne fait délibérément pas partie de cette réponse.
#[derive(Debug, Serialize)]
pub struct HuePairResponse {
    pub adapter_id: String,
    pub discovered_devices: usize,
}

#[derive(Debug, Serialize)]
pub struct HueBridgeCandidate {
    pub name: String,
    pub host: String,
    pub addresses: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MatterCommissionRequest {
    pub setup_code: String,
}
#[derive(Debug, Serialize)]
pub struct MatterCommissionResponse {
    pub job_id: String,
}
#[derive(Debug, Serialize)]
pub struct MatterCommissionJobResponse {
    pub job: CommissioningJob,
}

#[derive(Debug, Serialize)]
pub struct BackupResponse {
    pub manifest_version: u16,
    pub created_at: String,
    pub database_file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamClientMessage {
    Subscribe {
        topics: Vec<String>,
        after: Option<u64>,
    },
    Ack {
        id: u64,
    },
    Ping,
    Unsubscribe,
}

#[allow(clippy::large_enum_variant)] // Les messages sont immédiatement sérialisés sur le réseau.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamServerMessage {
    Ready {
        cursor: u64,
    },
    Event {
        #[serde(flatten)]
        event: StreamEvent,
    },
    ResyncRequired,
}

/// Enveloppe réseau stable : les données métier restent dans `data`, tandis
/// que le curseur et le type sont toujours au premier niveau du message.
#[derive(Debug, Serialize)]
pub struct StreamEvent {
    pub id: u64,
    pub topic: &'static str,
    pub event_type: &'static str,
    pub occurred_at: String,
    pub correlation_id: Option<String>,
    pub data_version: u8,
    pub data: Value,
}

/// Page de rejeu HTTP. Les éléments utilisent exactement l'enveloppe que le
/// WebSocket pousse, afin que les clients aient un seul décodeur d'événements.
#[derive(Debug, Serialize)]
pub struct EventPage {
    pub events: Vec<StreamEvent>,
    pub next_cursor: u64,
}

impl From<EventEnvelope> for StreamEvent {
    fn from(event: EventEnvelope) -> Self {
        let topic = event.data.topic();
        let event_type = event.data.event_type();
        let mut data = serde_json::to_value(event.data).expect("domain events serialize");
        if let Value::Object(data) = &mut data {
            data.remove("event_type");
        }
        Self {
            id: event.sequence,
            topic,
            event_type,
            occurred_at: event.occurred_at.to_rfc3339(),
            correlation_id: event.correlation_id,
            data_version: 1,
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use robine_domain::{Area, AreaId};

    #[test]
    fn websocket_event_uses_the_documented_flat_envelope() {
        let message = StreamServerMessage::Event {
            event: EventEnvelope {
                sequence: 42,
                occurred_at: Utc::now(),
                correlation_id: Some("cor_test".into()),
                data: robine_domain::EventData::AreaCreated {
                    area: Area {
                        id: AreaId::new(),
                        name: "Salon".into(),
                    },
                },
            }
            .into(),
        };
        let json = serde_json::to_value(message).unwrap();
        assert_eq!(json["type"], "event");
        assert_eq!(json["id"], 42);
        assert_eq!(json["topic"], "area");
        assert_eq!(json["event_type"], "area.created");
        assert!(json["data"].get("event_type").is_none());
    }

    #[test]
    fn http_replay_page_reuses_the_stream_event_contract() {
        let page = EventPage {
            events: vec![
                EventEnvelope {
                    sequence: 7,
                    occurred_at: Utc::now(),
                    correlation_id: None,
                    data: robine_domain::EventData::AreaCreated {
                        area: Area {
                            id: AreaId::new(),
                            name: "Bureau".into(),
                        },
                    },
                }
                .into(),
            ],
            next_cursor: 7,
        };
        let json = serde_json::to_value(page).unwrap();
        assert_eq!(json["next_cursor"], 7);
        assert_eq!(json["events"][0]["id"], 7);
        assert_eq!(json["events"][0]["event_type"], "area.created");
    }
}
