//! DTO versionnés, partagés par les transports HTTP et WebSocket.

use robine_domain::{CommandId, EventEnvelope, StateValue};
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
            "/api/v1/devices": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Devices" }, "401": { "description": "Authentication required" } } } },
            "/api/v1/entities/{id}": { "get": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }], "responses": { "200": { "description": "Entity detail" }, "404": { "description": "Entity not found" } } } },
            "/api/v1/entities/{id}/commands": { "post": { "security": [{ "bearerAuth": [] }], "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }, { "name": "Idempotency-Key", "in": "header", "required": true, "schema": { "type": "string", "minLength": 1 } }], "requestBody": { "required": true, "content": { "application/json": { "schema": command_request_schema() } } }, "responses": { "202": { "description": "Command accepted" } } } },
            "/api/v1/adapters/hue/pair": { "post": { "security": [{ "bearerAuth": [] }], "requestBody": { "required": true, "content": { "application/json": { "schema": { "type": "object", "required": ["authority", "certificate_pem", "certificate_sha256"], "properties": { "authority": { "type": "string" }, "certificate_pem": { "type": "string", "writeOnly": true }, "certificate_sha256": { "type": "string", "pattern": "^[a-f0-9]{64}$" } }, "additionalProperties": false } } } }, "responses": { "201": { "description": "Bridge paired and synchronized" }, "409": { "description": "Bridge button was not pressed" } } } },
            "/api/v1/adapters/hue/discover": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "200": { "description": "Locally discovered Hue bridges" } } } },
            "/api/v1/stream": { "get": { "security": [{ "bearerAuth": [] }], "responses": { "101": { "description": "Authenticated WebSocket upgrade" } } } }
        },
        "components": { "securitySchemes": { "bearerAuth": { "type": "http", "scheme": "bearer" } }, "schemas": { "CommandRequest": command_request_schema(), "WebSocketClientMessage": websocket_client_schema(), "WebSocketServerMessage": websocket_server_schema() } }
    })
}

pub fn command_request_schema() -> Value {
    json!({ "type": "object", "required": ["key", "value"], "properties": { "key": { "type": "string", "minLength": 1 }, "value": {} }, "additionalProperties": false })
}
pub fn websocket_client_schema() -> Value {
    json!({ "oneOf": [ { "type": "object", "properties": { "type": { "const": "subscribe" }, "topics": { "type": "array", "items": { "enum": ["state", "device", "automation", "adapter", "command"] } }, "after": { "type": "integer", "minimum": 0 } }, "required": ["type", "topics"], "additionalProperties": false }, { "type": "object", "properties": { "type": { "const": "ack" }, "id": { "type": "integer", "minimum": 1 } }, "required": ["type", "id"], "additionalProperties": false } ] })
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
}
#[derive(Debug, Serialize)]
pub struct McpTokenResponse {
    pub token: String,
    pub expires_at: String,
    pub scopes: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct CreateAreaRequest {
    pub name: String,
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
        event: EventEnvelope,
    },
    ResyncRequired,
}
