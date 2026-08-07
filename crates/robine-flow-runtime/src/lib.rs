//! Interpréteur déterministe de plans Flow. La planification persistante reste
//! au runtime hôte ; cette crate n'exécute aucun accès réseau ou SQLite.

use robine_domain::StateValue;
use robine_flow_plan::{ExecutionPlan, PlannedAction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RunId(pub Uuid);
impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}
impl RunId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

pub trait CommandGateway: Send + Sync {
    fn request(
        &self,
        entity_id: &str,
        key: &str,
        value: StateValue,
        idempotency_key: String,
    ) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RunTrace {
    pub steps: Vec<TraceStep>,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceStep {
    CommandRequested { entity_id: String, verb: String },
    Audit { message: String },
    Waiting { milliseconds: u64 },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunResult {
    Completed(RunTrace),
    Suspended {
        after_milliseconds: u64,
        next_action: usize,
        trace: RunTrace,
    },
}

pub fn execute(
    plan: &ExecutionPlan,
    run_id: RunId,
    gateway: &dyn CommandGateway,
) -> Result<RunResult, RuntimeError> {
    execute_from(plan, run_id, gateway, 0)
}

/// Reprend un plan au premier index non terminé. Les clés d'idempotence restent
/// stables pour un même `RunId`, donc une reprise ne réémet pas une intention.
pub fn execute_from(
    plan: &ExecutionPlan,
    run_id: RunId,
    gateway: &dyn CommandGateway,
    start_action: usize,
) -> Result<RunResult, RuntimeError> {
    let mut trace = RunTrace { steps: Vec::new() };
    for (index, action) in plan.actions.iter().enumerate().skip(start_action) {
        match action {
            PlannedAction::Command { entity_id, verb } => {
                let value = match verb.as_str() {
                    "turn-on" => StateValue::Bool(true),
                    "turn-off" => StateValue::Bool(false),
                    _ => return Err(RuntimeError::UnsupportedVerb(verb.clone())),
                };
                gateway
                    .request(
                        entity_id,
                        "switch",
                        value,
                        format!("flow:{}/{}", run_id.0, index),
                    )
                    .map_err(RuntimeError::Command)?;
                trace.steps.push(TraceStep::CommandRequested {
                    entity_id: entity_id.clone(),
                    verb: verb.clone(),
                });
            }
            PlannedAction::Audit { message } => trace.steps.push(TraceStep::Audit {
                message: message.clone(),
            }),
            PlannedAction::Wait { milliseconds } => {
                trace.steps.push(TraceStep::Waiting {
                    milliseconds: *milliseconds,
                });
                return Ok(RunResult::Suspended {
                    after_milliseconds: *milliseconds,
                    next_action: index + 1,
                    trace,
                });
            }
        }
    }
    Ok(RunResult::Completed(trace))
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("unsupported Flow command verb {0}")]
    UnsupportedVerb(String),
    #[error("command failed: {0}")]
    Command(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_flow_plan::{ExecutionPlan, PlannedAction};
    use std::sync::Mutex;
    struct Fake(Mutex<Vec<String>>);
    impl CommandGateway for Fake {
        fn request(&self, entity: &str, _: &str, _: StateValue, _: String) -> Result<(), String> {
            self.0.lock().unwrap().push(entity.into());
            Ok(())
        }
    }
    #[test]
    fn executes_until_a_persistable_wait() {
        let gateway = Fake(Mutex::new(Vec::new()));
        let plan = ExecutionPlan {
            actions: vec![
                PlannedAction::Command {
                    entity_id: "e1".into(),
                    verb: "turn-on".into(),
                },
                PlannedAction::Wait { milliseconds: 200 },
            ],
        };
        assert!(matches!(
            execute(&plan, RunId::new(), &gateway).unwrap(),
            RunResult::Suspended {
                after_milliseconds: 200,
                ..
            }
        ));
        assert_eq!(*gateway.0.lock().unwrap(), vec!["e1"]);
    }
}
