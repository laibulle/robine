//! Types purs du serveur MCP Robine (Streamable HTTP, 2025-11-25).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn result(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
    Control,
    AutomationWrite,
    Admin,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scopes(BTreeSet<Scope>);
impl Scopes {
    pub fn new(scopes: impl IntoIterator<Item = Scope>) -> Self {
        Self(scopes.into_iter().collect())
    }
    pub fn contains(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none", rename = "annotations")]
    pub annotations: Option<ToolAnnotations>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolAnnotations {
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: bool,
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: bool,
}

pub fn tool_definitions(scopes: &Scopes) -> Vec<ToolDefinition> {
    let mut tools = vec![
        read_tool(
            "robine.home.summary",
            "Résume l'état et la santé de la maison.",
            json!({ "type": "object", "additionalProperties": false }),
        ),
        read_tool(
            "robine.devices.list",
            "Liste les appareils Robine.",
            json!({ "type": "object", "properties": { "cursor": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }, "additionalProperties": false }),
        ),
        read_tool(
            "robine.entities.get",
            "Lit les capacités et l'état d'une entité explicite.",
            entity_schema(),
        ),
        read_tool(
            "robine.history.query",
            "Lit un historique borné d'une entité explicite.",
            entity_schema(),
        ),
        read_tool(
            "robine.automations.list",
            "Liste les automatisations et leurs dernières exécutions.",
            json!({ "type": "object", "additionalProperties": false }),
        ),
        read_tool(
            "robine.automation.explain",
            "Explique une exécution d'automatisation explicite.",
            json!({ "type": "object", "properties": { "run_id": { "type": "string", "minLength": 1 } }, "required": ["run_id"], "additionalProperties": false }),
        ),
        read_tool(
            "robine.automation.simulate",
            "Simule une automatisation sans effet de bord.",
            json!({ "type": "object", "properties": { "flow_id": { "type": "string", "minLength": 1 } }, "required": ["flow_id"], "additionalProperties": false }),
        ),
    ];
    if scopes.contains(Scope::Control) {
        tools.push(ToolDefinition { name: "robine.command.request", description: "Demande une commande sur une entité explicite après approbation.", input_schema: json!({ "type": "object", "properties": { "entity_id": { "type": "string", "minLength": 1 }, "key": { "type": "string" }, "value": {}, "approval_id": { "type": "string", "minLength": 1 } }, "required": ["entity_id", "key", "value", "approval_id"], "additionalProperties": false }), annotations: Some(ToolAnnotations { read_only_hint: false, destructive_hint: true }) });
    }
    if scopes.contains(Scope::AutomationWrite) {
        tools.push(ToolDefinition { name: "robine.automation.set-enabled", description: "Active ou désactive une automatisation explicite après approbation.", input_schema: json!({ "type": "object", "properties": { "flow_id": { "type": "string", "minLength": 1 }, "enabled": { "type": "boolean" }, "approval_id": { "type": "string", "minLength": 1 } }, "required": ["flow_id", "enabled", "approval_id"], "additionalProperties": false }), annotations: Some(ToolAnnotations { read_only_hint: false, destructive_hint: true }) });
    }
    tools.sort_by_key(|tool| tool.name);
    tools
}

fn read_tool(name: &'static str, description: &'static str, input_schema: Value) -> ToolDefinition {
    ToolDefinition {
        name,
        description,
        input_schema,
        annotations: Some(ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
        }),
    }
}
fn entity_schema() -> Value {
    json!({ "type": "object", "properties": { "entity_id": { "type": "string", "minLength": 1 } }, "required": ["entity_id"], "additionalProperties": false })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_token_does_not_discover_control_tools() {
        let tools = tool_definitions(&Scopes::new([Scope::Read]));
        assert!(
            tools
                .iter()
                .all(|tool| tool.name != "robine.command.request")
        );
        assert!(tools.windows(2).all(|pair| pair[0].name < pair[1].name));
    }
}
