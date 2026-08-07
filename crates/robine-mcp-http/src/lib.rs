//! Transport MCP Streamable HTTP. Les outils restent dans `robine-mcp-tools`.

use actix_web::{
    HttpRequest, HttpResponse,
    http::{StatusCode, header},
    web,
};
use robine_domain::StateValue;
use robine_mcp_tools::{McpTools, ToolError};
use robine_mcp_types::{
    JsonRpcRequest, JsonRpcResponse, McpPrincipal, McpWritePolicy, PROTOCOL_VERSION, Scope,
    approval_arguments_hash, tool_definitions_for,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
use thiserror::Error;

pub trait McpAuthenticator: Send + Sync {
    fn authenticate(&self, bearer: &str) -> Result<McpPrincipal, AuthenticationError>;
}

/// Le transport ne connaît pas SQLite : l'infrastructure fournit une
/// consommation transactionnelle liée au sujet MCP authentifié.
pub trait McpApprovalAuthorizer: Send + Sync {
    fn consume(
        &self,
        principal: &McpPrincipal,
        tool: &str,
        arguments_hash: &str,
        approval_id: &str,
    ) -> Result<bool, ApprovalError>;

    fn claim_allow_listed_command(
        &self,
        principal: &McpPrincipal,
        tool: &str,
        arguments_hash: &str,
        max_commands_per_hour: u32,
    ) -> Result<bool, ApprovalError>;
}

#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("invalid MCP token")]
    Invalid,
}

#[derive(Debug, Error)]
pub enum ApprovalError {
    #[error("MCP approval is unavailable")]
    Unavailable,
}

#[derive(Clone)]
pub struct McpHttpState {
    tools: McpTools,
    authenticator: Arc<dyn McpAuthenticator>,
    approvals: Arc<dyn McpApprovalAuthorizer>,
    allowed_origins: Arc<BTreeSet<String>>,
}

impl McpHttpState {
    pub fn new(
        tools: McpTools,
        authenticator: Arc<dyn McpAuthenticator>,
        approvals: Arc<dyn McpApprovalAuthorizer>,
        allowed_origins: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            tools,
            authenticator,
            approvals,
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
    let principal = match authorize(&request, &state) {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    HttpResponse::Ok().json(
        json!({ "protocolVersion": PROTOCOL_VERSION, "tools": implemented_tools(&principal) }),
    )
}

async fn handle_post(
    request: HttpRequest,
    state: web::Data<McpHttpState>,
    body: web::Json<JsonRpcRequest>,
) -> HttpResponse {
    let principal = match authorize(&request, &state) {
        Ok(principal) => principal,
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
        "tools/list" => JsonRpcResponse::result(
            request.id,
            json!({ "tools": implemented_tools(&principal) }),
        ),
        "tools/call" => tool_call(
            request.id,
            request.params.unwrap_or_else(|| json!({})),
            &state.tools,
            &principal,
            state.approvals.as_ref(),
        ),
        "resources/list" => {
            JsonRpcResponse::result(request.id, json!({ "resources": resources() }))
        }
        "resources/read" => resource_read(
            request.id,
            request.params.unwrap_or_else(|| json!({})),
            &state.tools,
        ),
        "prompts/list" => JsonRpcResponse::result(request.id, json!({ "prompts": prompts() })),
        "prompts/get" => prompt_get(
            request.id,
            request.params.unwrap_or_else(|| json!({})),
            &state.tools,
        ),
        _ => JsonRpcResponse::error(request.id, -32601, "MCP method not found"),
    };
    rpc_response(response)
}

fn prompt_get(id: Option<Value>, params: Value, tools: &McpTools) -> JsonRpcResponse {
    match params.get("name").and_then(Value::as_str) {
        Some("robine.explain-home-status") => match tools.home_summary() {
            Ok(summary) => JsonRpcResponse::result(
                id,
                json!({ "description": "Explique l'état courant sans déclencher d'action.", "messages": [ { "role": "user", "content": { "type": "text", "text": format!("Explique avec calme l'état actuel de la maison Robine à partir de ce résumé vérifié : {summary}") } } ] }),
            ),
            Err(error) => JsonRpcResponse::error(id, -32602, error.to_string()),
        },
        Some("robine.explain-automation-run") => {
            let Some(run_id) = params
                .get("arguments")
                .and_then(Value::as_object)
                .and_then(|arguments| arguments.get("run_id"))
                .and_then(Value::as_str)
            else {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    "robine.explain-automation-run requires arguments.run_id",
                );
            };
            match tools.automation_explain(run_id) {
                Ok(trace) => JsonRpcResponse::result(
                    id,
                    json!({ "description": "Explique une exécution Flow vérifiée, sans déclencher d'action.", "messages": [ { "role": "user", "content": { "type": "text", "text": format!("Explique calmement cette exécution Flow Robine, en distinguant les étapes réalisées, suspendues ou expirées. Ne suppose rien au-delà de cette trace vérifiée : {trace}") } } ] }),
                ),
                Err(error) => JsonRpcResponse::error(id, -32602, error.to_string()),
            }
        }
        Some(_) => JsonRpcResponse::error(id, -32602, "prompt name is unknown"),
        None => JsonRpcResponse::error(id, -32602, "prompts/get requires a prompt name"),
    }
}

fn tool_call(
    id: Option<Value>,
    params: Value,
    tools: &McpTools,
    principal: &McpPrincipal,
    approvals: &dyn McpApprovalAuthorizer,
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
        "robine.devices.list" => {
            let cursor = arguments.get("cursor").and_then(Value::as_str);
            let limit = arguments.get("limit").and_then(Value::as_u64);
            tools.list_devices(cursor, limit)
        }
        "robine.entities.get" => arguments
            .get("entity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("entity_id is required".into()))
            .and_then(|entity_id| tools.entity_get(entity_id)),
        "robine.history.query" => arguments
            .get("entity_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("entity_id is required".into()))
            .and_then(|entity_id| {
                tools.history_query(
                    entity_id,
                    arguments.get("property").and_then(Value::as_str),
                    arguments.get("after").and_then(Value::as_u64),
                    arguments.get("limit").and_then(Value::as_u64),
                )
            }),
        "robine.automations.list" => tools.list_automations(),
        "robine.automation.explain" => arguments
            .get("run_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("run_id is required".into()))
            .and_then(|run_id| tools.automation_explain(run_id)),
        "robine.automation.simulate" => arguments
            .get("flow_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("flow_id is required".into()))
            .and_then(|flow_id| tools.simulate_automation(flow_id)),
        "robine.command.request" if principal.scopes.contains(Scope::Control) => {
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
                .filter(|id| !id.trim().is_empty());
            match (entity_id, key, value) {
                (Ok(entity_id), Ok(key), Ok(value)) => {
                    let mut approved_arguments = arguments.clone();
                    approved_arguments
                        .as_object_mut()
                        .expect("MCP arguments defaults to an object")
                        .remove("approval_id");
                    let arguments_hash = approval_arguments_hash(&approved_arguments);
                    if let Err(message) = authorize_command(
                        principal,
                        approvals,
                        name,
                        entity_id,
                        &key,
                        &arguments_hash,
                        approval_id,
                    ) {
                        return JsonRpcResponse::error(id, -32001, message);
                    }
                    let idempotency_reference = approval_id
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("allow-listed:{arguments_hash}"));
                    tools.request_command(entity_id, key, value, &idempotency_reference)
                }
                _ => return JsonRpcResponse::error(id, -32602, "invalid command arguments"),
            }
        }
        "robine.command.request" => {
            return JsonRpcResponse::error(id, -32604, "MCP scope robine:control is required");
        }
        "robine.automation.set-enabled" if principal.scopes.contains(Scope::AutomationWrite) => {
            let flow_id = arguments
                .get("flow_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::InvalidArguments("flow_id is required".into()));
            let enabled = arguments
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| ToolError::InvalidArguments("enabled must be a boolean".into()));
            let approval_id = arguments
                .get("approval_id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| ToolError::InvalidArguments("approval_id is required".into()));
            match (flow_id, enabled, approval_id) {
                (Ok(flow_id), Ok(enabled), Ok(approval_id)) => {
                    let mut approved_arguments = arguments.clone();
                    approved_arguments
                        .as_object_mut()
                        .expect("MCP arguments defaults to an object")
                        .remove("approval_id");
                    let arguments_hash = approval_arguments_hash(&approved_arguments);
                    match &principal.write_policy {
                        McpWritePolicy::ConfirmEach => {
                            match approvals.consume(principal, name, &arguments_hash, approval_id) {
                                Ok(true) => tools.set_automation_enabled(flow_id, enabled),
                                Ok(false) => {
                                    return JsonRpcResponse::error(
                                        id,
                                        -32001,
                                        "MCP approval is missing, expired, already consumed, or does not match this request",
                                    );
                                }
                                Err(_) => {
                                    return JsonRpcResponse::error(
                                        id,
                                        -32001,
                                        "MCP approval is unavailable",
                                    );
                                }
                            }
                        }
                        McpWritePolicy::AllowListed { .. } => {
                            return JsonRpcResponse::error(
                                id,
                                -32001,
                                "allow-listed MCP tokens cannot modify automations",
                            );
                        }
                        McpWritePolicy::ReadOnly => {
                            return JsonRpcResponse::error(
                                id,
                                -32001,
                                "MCP write policy denies this action",
                            );
                        }
                    }
                }
                _ => return JsonRpcResponse::error(id, -32602, "invalid automation arguments"),
            }
        }
        "robine.automation.set-enabled" => {
            return JsonRpcResponse::error(
                id,
                -32604,
                "MCP scope robine:automation:write is required",
            );
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
        _ if uri.starts_with("robine://devices/") => {
            tools.device_get(uri.trim_start_matches("robine://devices/"))
        }
        _ if uri.starts_with("robine://entities/") => {
            tools.entity_get(uri.trim_start_matches("robine://entities/"))
        }
        _ if uri.starts_with("robine://automations/") => {
            tools.automation_get(uri.trim_start_matches("robine://automations/"))
        }
        _ if uri.starts_with("robine://automation-runs/") => {
            tools.automation_explain(uri.trim_start_matches("robine://automation-runs/"))
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

fn authorize_command(
    principal: &McpPrincipal,
    approvals: &dyn McpApprovalAuthorizer,
    tool: &str,
    entity_id: &str,
    key: &str,
    arguments_hash: &str,
    approval_id: Option<&str>,
) -> Result<(), &'static str> {
    match &principal.write_policy {
        McpWritePolicy::ConfirmEach => {
            let approval_id =
                approval_id.ok_or("MCP approval_id is required by this token policy")?;
            match approvals.consume(principal, tool, arguments_hash, approval_id) {
                Ok(true) => Ok(()),
                Ok(false) => Err(
                    "MCP approval is missing, expired, already consumed, or does not match this request",
                ),
                Err(_) => Err("MCP approval is unavailable"),
            }
        }
        McpWritePolicy::AllowListed { .. } => {
            if !principal.write_policy.permits_command(entity_id, key) {
                return Err("MCP allow-list denies this entity or property");
            }
            let max_commands_per_hour = principal
                .write_policy
                .max_commands_per_hour()
                .expect("allow-listed policy has a quota");
            match approvals.claim_allow_listed_command(
                principal,
                tool,
                arguments_hash,
                max_commands_per_hour,
            ) {
                Ok(true) => Ok(()),
                Ok(false) => Err("MCP allow-list hourly command quota has been reached"),
                Err(_) => Err("MCP allow-list quota is unavailable"),
            }
        }
        McpWritePolicy::ReadOnly => Err("MCP write policy denies this action"),
    }
}

fn implemented_tools(principal: &McpPrincipal) -> Vec<robine_mcp_types::ToolDefinition> {
    let names = [
        "robine.home.summary",
        "robine.devices.list",
        "robine.entities.get",
        "robine.history.query",
        "robine.automations.list",
        "robine.automation.explain",
        "robine.automation.simulate",
        "robine.automation.set-enabled",
        "robine.command.request",
    ];
    tool_definitions_for(&principal.scopes, &principal.write_policy)
        .into_iter()
        .filter(|tool| names.contains(&tool.name))
        .collect()
}
fn resources() -> Vec<Value> {
    vec![
        json!({ "uri": "robine://home/summary", "name": "Home summary", "mimeType": "application/json" }),
        json!({ "uriTemplate": "robine://devices/{device_id}", "name": "Device", "mimeType": "application/json" }),
        json!({ "uriTemplate": "robine://entities/{entity_id}", "name": "Entity", "mimeType": "application/json" }),
        json!({ "uriTemplate": "robine://automations/{flow_id}", "name": "Automation", "mimeType": "application/json" }),
        json!({ "uriTemplate": "robine://automation-runs/{run_id}", "name": "Automation run", "mimeType": "application/json" }),
    ]
}
fn prompts() -> Vec<Value> {
    vec![
        json!({ "name": "robine.explain-home-status", "description": "Prépare une lecture de l'état de la maison." }),
        json!({
            "name": "robine.explain-automation-run",
            "description": "Prépare l'explication d'une exécution Flow persistée.",
            "arguments": [{ "name": "run_id", "description": "Identifiant de l'exécution Flow.", "required": true }]
        }),
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

fn authorize(request: &HttpRequest, state: &McpHttpState) -> Result<McpPrincipal, HttpResponse> {
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
    use robine_mcp_types::Scopes;

    struct TestAuthorizer {
        allow_quota: bool,
    }

    impl McpApprovalAuthorizer for TestAuthorizer {
        fn consume(
            &self,
            _principal: &McpPrincipal,
            _tool: &str,
            _arguments_hash: &str,
            _approval_id: &str,
        ) -> Result<bool, ApprovalError> {
            Ok(false)
        }

        fn claim_allow_listed_command(
            &self,
            _principal: &McpPrincipal,
            _tool: &str,
            _arguments_hash: &str,
            _max_commands_per_hour: u32,
        ) -> Result<bool, ApprovalError> {
            Ok(self.allow_quota)
        }
    }
    #[test]
    fn read_only_tools_do_not_include_control() {
        let principal = McpPrincipal {
            token_id: "test".into(),
            scopes: Scopes::new([Scope::Read]),
            write_policy: McpWritePolicy::ReadOnly,
        };
        let names = implemented_tools(&principal)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(!names.contains(&"robine.command.request"));
        assert!(names.contains(&"robine.automations.list"));
        assert!(names.contains(&"robine.automation.explain"));
        assert!(names.contains(&"robine.automation.simulate"));
    }

    #[test]
    fn allow_listed_commands_require_an_exact_target_and_available_quota() {
        let principal = McpPrincipal {
            token_id: "test".into(),
            scopes: Scopes::new([Scope::Read, Scope::Control]),
            write_policy: McpWritePolicy::AllowListed {
                commands: vec![robine_mcp_types::McpCommandAllowance {
                    entity_id: "entity-1".into(),
                    keys: vec!["switch".into()],
                }],
                max_commands_per_hour: 1,
            },
        };
        assert!(
            authorize_command(
                &principal,
                &TestAuthorizer { allow_quota: true },
                "robine.command.request",
                "entity-1",
                "switch",
                &"a".repeat(64),
                None,
            )
            .is_ok()
        );
        assert!(
            authorize_command(
                &principal,
                &TestAuthorizer { allow_quota: true },
                "robine.command.request",
                "entity-1",
                "brightness",
                &"a".repeat(64),
                None,
            )
            .is_err()
        );
        assert!(
            authorize_command(
                &principal,
                &TestAuthorizer { allow_quota: false },
                "robine.command.request",
                "entity-1",
                "switch",
                &"a".repeat(64),
                None,
            )
            .unwrap_err()
            .contains("quota")
        );
    }

    #[test]
    fn automation_explanation_prompt_requires_a_run_id() {
        let catalog = prompts();
        let automation = catalog
            .iter()
            .find(|prompt| prompt["name"] == "robine.explain-automation-run")
            .expect("automation explanation prompt");
        assert_eq!(automation["arguments"][0]["name"], "run_id");
        assert_eq!(automation["arguments"][0]["required"], true);
    }
}
