//! Transport MCP Streamable HTTP. Les outils restent dans `robine-mcp-tools`.

use actix_web::{
    HttpRequest, HttpResponse,
    http::{StatusCode, header},
    web,
};
use robine_domain::StateValue;
use robine_mcp_tools::{McpTools, ToolError};
use robine_mcp_types::{
    JsonRpcRequest, JsonRpcResponse, PROTOCOL_VERSION, Scope, Scopes, tool_definitions,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use thiserror::Error;

pub trait McpAuthenticator: Send + Sync {
    fn authenticate(&self, bearer: &str) -> Result<Scopes, AuthenticationError>;
}

#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("invalid MCP token")]
    Invalid,
}

#[derive(Clone)]
pub struct McpHttpState {
    tools: McpTools,
    authenticator: Arc<dyn McpAuthenticator>,
    allowed_origins: Arc<BTreeSet<String>>,
}

impl McpHttpState {
    pub fn new(
        tools: McpTools,
        authenticator: Arc<dyn McpAuthenticator>,
        allowed_origins: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            tools,
            authenticator,
            allowed_origins: Arc::new(allowed_origins.into_iter().collect()),
        }
    }
}

pub fn configure(configuration: &mut web::ServiceConfig, state: McpHttpState) {
    configuration
        .app_data(web::Data::new(state))
        .route("/mcp", web::post().to(handle_post))
        .route("/mcp", web::get().to(handle_get));
}

async fn handle_get(request: HttpRequest, state: web::Data<McpHttpState>) -> HttpResponse {
    let scopes = match authorize(&request, &state) {
        Ok(scopes) => scopes,
        Err(response) => return response,
    };
    HttpResponse::Ok()
        .json(json!({ "protocolVersion": PROTOCOL_VERSION, "tools": implemented_tools(&scopes) }))
}

async fn handle_post(
    request: HttpRequest,
    state: web::Data<McpHttpState>,
    body: web::Json<JsonRpcRequest>,
) -> HttpResponse {
    let scopes = match authorize(&request, &state) {
        Ok(scopes) => scopes,
        Err(response) => return response,
    };
    let request = body.into_inner();
    if request.jsonrpc != "2.0" {
        return rpc_response(JsonRpcResponse::error(
            request.id,
            -32600,
            "JSON-RPC version must be 2.0",
        ));
    }
    let response = match request.method.as_str() {
        "initialize" => JsonRpcResponse::result(
            request.id,
            json!({ "protocolVersion": PROTOCOL_VERSION, "capabilities": { "tools": { "listChanged": false }, "resources": {}, "prompts": {} }, "serverInfo": { "name": "robine", "version": env!("CARGO_PKG_VERSION") } }),
        ),
        "tools/list" => {
            JsonRpcResponse::result(request.id, json!({ "tools": implemented_tools(&scopes) }))
        }
        "tools/call" => tool_call(
            request.id,
            request.params.unwrap_or_else(|| json!({})),
            &state.tools,
            &scopes,
        ),
        "resources/list" => {
            JsonRpcResponse::result(request.id, json!({ "resources": resources() }))
        }
        "resources/read" => resource_read(
            request.id,
            request.params.unwrap_or_else(|| json!({})),
            &state.tools,
        ),
        "prompts/list" => JsonRpcResponse::result(
            request.id,
            json!({ "prompts": [ { "name": "robine.explain-home-status", "description": "Prépare une lecture de l'état de la maison." } ] }),
        ),
        _ => JsonRpcResponse::error(request.id, -32601, "MCP method not found"),
    };
    rpc_response(response)
}

fn tool_call(
    id: Option<Value>,
    params: Value,
    tools: &McpTools,
    scopes: &Scopes,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return JsonRpcResponse::error(id, -32602, "tools/call requires a tool name");
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match name {
        "robine.home.summary" => tools.home_summary(),
        "robine.devices.list" => tools.list_devices(),
        "robine.entities.get" => arguments
            .get("entity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("entity_id is required".into()))
            .and_then(|entity_id| tools.entity_get(entity_id)),
        "robine.history.query" => arguments
            .get("entity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("entity_id is required".into()))
            .and_then(|entity_id| tools.history_query(entity_id)),
        "robine.command.request" if scopes.contains(Scope::Control) => {
            let entity_id = arguments
                .get("entity_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArguments("entity_id is required".into()));
            let key = arguments
                .get("key")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| ToolError::InvalidArguments("key is required".into()));
            let value = arguments
                .get("value")
                .cloned()
                .ok_or_else(|| ToolError::InvalidArguments("value is required".into()))
                .and_then(|value| {
                    serde_json::from_value::<StateValue>(value)
                        .map_err(|_| ToolError::InvalidArguments("value is invalid".into()))
                });
            let approval_id = arguments
                .get("approval_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArguments("approval_id is required".into()));
            match (entity_id, key, value, approval_id) {
                (Ok(entity_id), Ok(key), Ok(value), Ok(approval_id)) => {
                    tools.request_command(entity_id, key, value, approval_id)
                }
                _ => return JsonRpcResponse::error(id, -32602, "invalid command arguments"),
            }
        }
        "robine.command.request" => {
            return JsonRpcResponse::error(id, -32604, "MCP scope robine:control is required");
        }
        _ => return JsonRpcResponse::error(id, -32601, "MCP tool not found"),
    };
    match result {
        Ok(value) => JsonRpcResponse::result(id, tool_result(value)),
        Err(error) => JsonRpcResponse::error(id, -32602, error.to_string()),
    }
}

fn resource_read(id: Option<Value>, params: Value, tools: &McpTools) -> JsonRpcResponse {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return JsonRpcResponse::error(id, -32602, "resources/read requires a URI");
    };
    let value = match uri {
        "robine://home/summary" => tools.home_summary(),
        _ if uri.starts_with("robine://entities/") => {
            tools.entity_get(uri.trim_start_matches("robine://entities/"))
        }
        _ => return JsonRpcResponse::error(id, -32602, "resource URI is unknown"),
    };
    match value {
        Ok(value) => JsonRpcResponse::result(
            id,
            json!({ "contents": [ { "uri": uri, "mimeType": "application/json", "text": value.to_string() } ] }),
        ),
        Err(error) => JsonRpcResponse::error(id, -32602, error.to_string()),
    }
}

fn implemented_tools(scopes: &Scopes) -> Vec<robine_mcp_types::ToolDefinition> {
    let names = [
        "robine.home.summary",
        "robine.devices.list",
        "robine.entities.get",
        "robine.history.query",
        "robine.command.request",
    ];
    tool_definitions(scopes)
        .into_iter()
        .filter(|tool| names.contains(&tool.name))
        .collect()
}
fn resources() -> Vec<Value> {
    vec![
        json!({ "uri": "robine://home/summary", "name": "Home summary", "mimeType": "application/json" }),
        json!({ "uriTemplate": "robine://entities/{entity_id}", "name": "Entity", "mimeType": "application/json" }),
    ]
}
fn tool_result(value: Value) -> Value {
    json!({ "content": [ { "type": "text", "text": value.to_string() } ], "structuredContent": value })
}
fn rpc_response(response: JsonRpcResponse) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .json(response)
}

fn authorize(request: &HttpRequest, state: &McpHttpState) -> Result<Scopes, HttpResponse> {
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if !state.allowed_origins.contains(origin) {
            return Err(HttpResponse::build(StatusCode::FORBIDDEN).finish());
        }
    }
    let Some(token) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(HttpResponse::build(StatusCode::UNAUTHORIZED).finish());
    };
    state
        .authenticator
        .authenticate(token)
        .map_err(|_| HttpResponse::build(StatusCode::UNAUTHORIZED).finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_tools_do_not_include_control() {
        let names = implemented_tools(&Scopes::new([Scope::Read]))
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(!names.contains(&"robine.command.request"));
    }
}
