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

impl Scope {
    pub const fn as_storage(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Control => "control",
            Self::AutomationWrite => "automation_write",
            Self::Admin => "admin",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "control" => Some(Self::Control),
            "automation_write" => Some(Self::AutomationWrite),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
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

    pub fn iter(&self) -> impl Iterator<Item = Scope> + '_ {
        self.0.iter().copied()
    }
}

/// Sujet stable d'un appel MCP authentifié. L'identifiant ne révèle jamais le
/// secret bearer et permet de lier une approbation à un seul jeton.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpPrincipal {
    pub token_id: String,
    pub scopes: Scopes,
    pub write_policy: McpWritePolicy,
}

/// Politique d'écriture choisie lors de l'émission d'un jeton MCP. Un jeton
/// qui ne porte aucun scope d'écriture reste explicitement `ReadOnly`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum McpWritePolicy {
    ReadOnly,
    ConfirmEach,
    AllowListed {
        commands: Vec<McpCommandAllowance>,
        max_commands_per_hour: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpCommandAllowance {
    pub entity_id: String,
    pub keys: Vec<String>,
}

impl McpWritePolicy {
    pub fn default_for(scopes: &Scopes) -> Self {
        if scopes.contains(Scope::Control) || scopes.contains(Scope::AutomationWrite) {
            Self::ConfirmEach
        } else {
            Self::ReadOnly
        }
    }

    pub fn validate_for(&self, scopes: &Scopes) -> Result<(), &'static str> {
        let has_write_scope = scopes.contains(Scope::Control) || scopes.contains(Scope::AutomationWrite);
        match self {
            Self::ReadOnly if has_write_scope => Err("a write scope requires a write policy"),
            Self::ReadOnly => Ok(()),
            Self::ConfirmEach if has_write_scope => Ok(()),
            Self::ConfirmEach => Err("confirm_each requires a write scope"),
            Self::AllowListed { commands, max_commands_per_hour } => {
                if !scopes.contains(Scope::Control) {
                    return Err("allow_listed requires the control scope");
                }
                if !(1..=3_600).contains(max_commands_per_hour) {
                    return Err("max_commands_per_hour must be between 1 and 3600");
                }
                if commands.is_empty() {
                    return Err("an allow_listed policy requires at least one command");
                }
                let mut seen = BTreeSet::new();
                for command in commands {
                    if command.entity_id.trim().is_empty() || command.keys.is_empty() {
                        return Err("each allow-listed command requires an entity and a key");
                    }
                    for key in &command.keys {
                        if key.trim().is_empty() || !seen.insert((command.entity_id.as_str(), key.as_str())) {
                            return Err("allow-listed commands must be unique and non-empty");
                        }
                    }
                }
                Ok(())
            }
        }
    }

    pub fn permits_command(&self, entity_id: &str, key: &str) -> bool {
        match self {
            Self::AllowListed { commands, .. } => commands.iter().any(|command| {
                command.entity_id == entity_id && command.keys.iter().any(|allowed| allowed == key)
            }),
            _ => false,
        }
    }

    pub fn max_commands_per_hour(&self) -> Option<u32> {
        match self {
            Self::AllowListed { max_commands_per_hour, .. } => Some(*max_commands_per_hour),
            _ => None,
        }
    }
}

/// Empreinte déterministe de la charge à approuver. Les objets JSON sont
/// ordonnés récursivement afin qu'un même appel garde la même empreinte,
/// indépendamment de l'ordre des clés fourni par le client.
pub fn approval_arguments_hash(arguments: &Value) -> String {
    use sha2::{Digest, Sha256};

    fn canonical(value: &Value) -> String {
        match value {
            Value::Null => "null".into(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::String(value) => serde_json::to_string(value).expect("string serializes"),
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(canonical).collect::<Vec<_>>().join(",")
            ),
            Value::Object(values) => {
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_unstable_by_key(|(key, _)| *key);
                format!(
                    "{{{}}}",
                    entries
                        .into_iter()
                        .map(|(key, value)| format!(
                            "{}:{}",
                            serde_json::to_string(key).expect("key serializes"),
                            canonical(value)
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }

    format!("{:x}", Sha256::digest(canonical(arguments).as_bytes()))
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
    tool_definitions_for(scopes, &McpWritePolicy::default_for(scopes))
}

pub fn tool_definitions_for(
    scopes: &Scopes,
    write_policy: &McpWritePolicy,
) -> Vec<ToolDefinition> {
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
        let approval_required = !matches!(write_policy, McpWritePolicy::AllowListed { .. });
        let mut required = vec!["entity_id", "key", "value"];
        if approval_required {
            required.push("approval_id");
        }
        tools.push(ToolDefinition { name: "robine.command.request", description: if approval_required { "Demande une commande sur une entité explicite après approbation." } else { "Demande une commande explicitement autorisée sur une entité." }, input_schema: json!({ "type": "object", "properties": { "entity_id": { "type": "string", "minLength": 1 }, "key": { "type": "string" }, "value": {}, "approval_id": { "type": "string", "minLength": 1 } }, "required": required, "additionalProperties": false }), annotations: Some(ToolAnnotations { read_only_hint: false, destructive_hint: true }) });
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

    #[test]
    fn approval_hash_is_stable_when_json_object_key_order_changes() {
        assert_eq!(
            approval_arguments_hash(&json!({ "entity_id": "a", "value": { "b": 2, "a": 1 } })),
            approval_arguments_hash(&json!({ "value": { "a": 1, "b": 2 }, "entity_id": "a" })),
        );
    }

    #[test]
    fn allow_list_rejects_duplicate_targets_and_requires_control() {
        let policy = McpWritePolicy::AllowListed {
            commands: vec![McpCommandAllowance {
                entity_id: "entity-1".into(),
                keys: vec!["switch".into(), "switch".into()],
            }],
            max_commands_per_hour: 10,
        };
        assert!(policy.validate_for(&Scopes::new([Scope::Control])).is_err());
        assert!(policy.validate_for(&Scopes::new([Scope::Read])).is_err());
    }

    #[test]
    fn allow_listed_command_tool_does_not_require_a_per_call_approval() {
        let scopes = Scopes::new([Scope::Read, Scope::Control]);
        let policy = McpWritePolicy::AllowListed {
            commands: vec![McpCommandAllowance {
                entity_id: "entity-1".into(),
                keys: vec!["switch".into()],
            }],
            max_commands_per_hour: 4,
        };
        let command = tool_definitions_for(&scopes, &policy)
            .into_iter()
            .find(|tool| tool.name == "robine.command.request")
            .unwrap();
        assert!(!command.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "approval_id"));
    }
}
