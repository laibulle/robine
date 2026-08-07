//! Interpréteur déterministe de plans Flow. La planification persistante reste
//! au runtime hôte ; cette crate n'exécute aucun accès réseau ou SQLite.

use robine_domain::StateValue;
use robine_flow_plan::{AwaitTrigger, CommandConfirmation, ExecutionPlan, PlannedAction};
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
    ) -> Result<String, String>;
    fn set_automation_enabled(
        &self,
        flow_id: &str,
        enabled: bool,
        idempotency_key: String,
    ) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunTrace {
    pub steps: Vec<TraceStep>,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceStep {
    GuardEvaluated {
        passed: bool,
        summary: String,
    },
    CommandRequested {
        entity_id: String,
        verb: String,
    },
    RetryScheduled {
        next_attempt: u8,
        total_attempts: u8,
        backoff_milliseconds: u64,
    },
    RetryExhausted {
        attempts: u8,
    },
    ActionFailed {
        action: String,
    },
    Audit {
        message: String,
    },
    Waiting {
        milliseconds: u64,
    },
    Awaiting {
        trigger: AwaitTrigger,
        timeout_milliseconds: Option<u64>,
    },
    AutomationChanged {
        flow_id: String,
        enabled: bool,
    },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunResult {
    Completed(RunTrace),
    Failed(RunTrace),
    Skipped(RunTrace),
    TimedOut(RunTrace),
    Cancelled(RunTrace),
    Queued(RunTrace),
    Suspended {
        after_milliseconds: Option<u64>,
        next_action: usize,
        awaiting: Option<AwaitTrigger>,
        await_timeout_is_failure: bool,
        retry_attempt: Option<u8>,
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
    execute_from_trace(
        plan,
        run_id,
        gateway,
        start_action,
        RunTrace { steps: Vec::new() },
    )
}

/// Reprend une exécution durable sans perdre les décisions et actions déjà
/// tracées avant le dernier `wait` ou `await`.
pub fn execute_from_trace(
    plan: &ExecutionPlan,
    run_id: RunId,
    gateway: &dyn CommandGateway,
    start_action: usize,
    trace: RunTrace,
) -> Result<RunResult, RuntimeError> {
    execute_from_resumption(plan, run_id, gateway, start_action, None, trace)
}

/// Reprend aussi une tentative `retry` déjà suspendue par son backoff.
pub fn execute_from_resumption(
    plan: &ExecutionPlan,
    run_id: RunId,
    gateway: &dyn CommandGateway,
    start_action: usize,
    retry_attempt: Option<u8>,
    mut trace: RunTrace,
) -> Result<RunResult, RuntimeError> {
    for (index, action) in plan.actions.iter().enumerate().skip(start_action) {
        match action {
            PlannedAction::Command {
                entity_id,
                verb,
                brightness,
                confirmation,
            } => {
                let value = match verb.as_str() {
                    "turn-on" => StateValue::Bool(true),
                    "turn-off" => StateValue::Bool(false),
                    _ => {
                        trace.steps.push(TraceStep::ActionFailed {
                            action: format!("unsupported command verb: {verb}"),
                        });
                        return Ok(RunResult::Failed(trace));
                    }
                };
                let command_id = match gateway.request(
                    entity_id,
                    "switch",
                    value,
                    format!("flow:{}/{}", run_id.0, index),
                ) {
                    Ok(command_id) => command_id,
                    Err(_) => {
                        trace.steps.push(TraceStep::ActionFailed {
                            action: format!("command {verb} failed"),
                        });
                        return Ok(RunResult::Failed(trace));
                    }
                };
                if let Some(brightness) = brightness {
                    if gateway
                        .request(
                            entity_id,
                            "light.brightness",
                            StateValue::Percentage(*brightness),
                            format!("flow:{}/{}:brightness", run_id.0, index),
                        )
                        .is_err()
                    {
                        trace.steps.push(TraceStep::ActionFailed {
                            action: "brightness command failed".into(),
                        });
                        return Ok(RunResult::Failed(trace));
                    }
                }
                trace.steps.push(TraceStep::CommandRequested {
                    entity_id: entity_id.clone(),
                    verb: verb.clone(),
                });
                if let CommandConfirmation::Reported {
                    timeout_milliseconds,
                } = confirmation
                {
                    let trigger = AwaitTrigger::CommandConfirmed { command_id };
                    trace.steps.push(TraceStep::Awaiting {
                        trigger: trigger.clone(),
                        timeout_milliseconds: Some(*timeout_milliseconds),
                    });
                    return Ok(RunResult::Suspended {
                        after_milliseconds: Some(*timeout_milliseconds),
                        next_action: index + 1,
                        awaiting: Some(trigger),
                        await_timeout_is_failure: true,
                        retry_attempt: None,
                        trace,
                    });
                }
            }
            PlannedAction::Audit { message } => trace.steps.push(TraceStep::Audit {
                message: message.clone(),
            }),
            PlannedAction::SetAutomationEnabled { flow_id, enabled } => {
                if gateway
                    .set_automation_enabled(
                        flow_id,
                        *enabled,
                        format!("flow:{}/{}:automation", run_id.0, index),
                    )
                    .is_err()
                {
                    trace.steps.push(TraceStep::ActionFailed {
                        action: "automation update failed".into(),
                    });
                    return Ok(RunResult::Failed(trace));
                }
                trace.steps.push(TraceStep::AutomationChanged {
                    flow_id: flow_id.clone(),
                    enabled: *enabled,
                });
            }
            PlannedAction::Wait { milliseconds } => {
                trace.steps.push(TraceStep::Waiting {
                    milliseconds: *milliseconds,
                });
                return Ok(RunResult::Suspended {
                    after_milliseconds: Some(*milliseconds),
                    next_action: index + 1,
                    awaiting: None,
                    await_timeout_is_failure: false,
                    retry_attempt: None,
                    trace,
                });
            }
            PlannedAction::Await {
                trigger,
                timeout_milliseconds,
            } => {
                trace.steps.push(TraceStep::Awaiting {
                    trigger: trigger.clone(),
                    timeout_milliseconds: *timeout_milliseconds,
                });
                return Ok(RunResult::Suspended {
                    after_milliseconds: *timeout_milliseconds,
                    next_action: index + 1,
                    awaiting: Some(trigger.clone()),
                    await_timeout_is_failure: false,
                    retry_attempt: None,
                    trace,
                });
            }
            PlannedAction::Retry {
                action,
                attempts,
                backoff_milliseconds,
            } => {
                let attempt = if index == start_action {
                    retry_attempt.unwrap_or(0)
                } else {
                    0
                };
                match execute_retryable_action(action, &run_id, gateway, index, attempt, &mut trace)
                {
                    Ok(()) => {}
                    Err(_) if attempt.saturating_add(1) < *attempts => {
                        let next_attempt = attempt.saturating_add(1);
                        trace.steps.push(TraceStep::RetryScheduled {
                            next_attempt: next_attempt.saturating_add(1),
                            total_attempts: *attempts,
                            backoff_milliseconds: *backoff_milliseconds,
                        });
                        return Ok(RunResult::Suspended {
                            after_milliseconds: Some(*backoff_milliseconds),
                            next_action: index,
                            awaiting: None,
                            await_timeout_is_failure: false,
                            retry_attempt: Some(next_attempt),
                            trace,
                        });
                    }
                    Err(_) => {
                        trace.steps.push(TraceStep::RetryExhausted {
                            attempts: *attempts,
                        });
                        return Ok(RunResult::Failed(trace));
                    }
                }
            }
        }
    }
    Ok(RunResult::Completed(trace))
}

fn execute_retryable_action(
    action: &PlannedAction,
    run_id: &RunId,
    gateway: &dyn CommandGateway,
    action_index: usize,
    attempt: u8,
    trace: &mut RunTrace,
) -> Result<(), RuntimeError> {
    let key = format!("flow:{}/{}:attempt:{}", run_id.0, action_index, attempt + 1);
    match action {
        PlannedAction::Command {
            entity_id,
            verb,
            brightness,
            ..
        } => {
            let value = match verb.as_str() {
                "turn-on" => StateValue::Bool(true),
                "turn-off" => StateValue::Bool(false),
                _ => return Err(RuntimeError::UnsupportedVerb(verb.clone())),
            };
            gateway
                .request(entity_id, "switch", value, key.clone())
                .map_err(RuntimeError::Command)?;
            if let Some(brightness) = brightness {
                gateway
                    .request(
                        entity_id,
                        "light.brightness",
                        StateValue::Percentage(*brightness),
                        format!("{key}:brightness"),
                    )
                    .map_err(RuntimeError::Command)?;
            }
            trace.steps.push(TraceStep::CommandRequested {
                entity_id: entity_id.clone(),
                verb: verb.clone(),
            });
            Ok(())
        }
        PlannedAction::Audit { message } => {
            trace.steps.push(TraceStep::Audit {
                message: message.clone(),
            });
            Ok(())
        }
        PlannedAction::SetAutomationEnabled { flow_id, enabled } => {
            gateway
                .set_automation_enabled(flow_id, *enabled, format!("{key}:automation"))
                .map_err(RuntimeError::Command)?;
            trace.steps.push(TraceStep::AutomationChanged {
                flow_id: flow_id.clone(),
                enabled: *enabled,
            });
            Ok(())
        }
        _ => Err(RuntimeError::InvalidRetryAction),
    }
}

impl RunResult {
    pub fn trace(&self) -> &RunTrace {
        match self {
            Self::Completed(trace)
            | Self::Failed(trace)
            | Self::Skipped(trace)
            | Self::TimedOut(trace)
            | Self::Cancelled(trace)
            | Self::Queued(trace) => trace,
            Self::Suspended { trace, .. } => trace,
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("unsupported Flow command verb {0}")]
    UnsupportedVerb(String),
    #[error("command failed: {0}")]
    Command(String),
    #[error("retry contains an action that cannot be retried")]
    InvalidRetryAction,
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_flow_plan::{ExecutionPlan, PlannedAction};
    use std::sync::Mutex;
    struct Fake(Mutex<Vec<(String, String, StateValue)>>);
    impl CommandGateway for Fake {
        fn request(
            &self,
            entity: &str,
            key: &str,
            value: StateValue,
            _: String,
        ) -> Result<String, String> {
            self.0
                .lock()
                .unwrap()
                .push((entity.into(), key.into(), value));
            Ok("command-1".into())
        }

        fn set_automation_enabled(
            &self,
            _flow_id: &str,
            _enabled: bool,
            _idempotency_key: String,
        ) -> Result<(), String> {
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
                    brightness: None,
                    confirmation: Default::default(),
                },
                PlannedAction::Wait { milliseconds: 200 },
            ],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        assert!(matches!(
            execute(&plan, RunId::new(), &gateway).unwrap(),
            RunResult::Suspended {
                after_milliseconds: Some(200),
                ..
            }
        ));
        assert_eq!(
            *gateway.0.lock().unwrap(),
            vec![("e1".into(), "switch".into(), StateValue::Bool(true))]
        );
    }

    #[test]
    fn executes_switch_and_brightness_from_one_planned_command() {
        let gateway = Fake(Mutex::new(Vec::new()));
        let plan = ExecutionPlan {
            actions: vec![PlannedAction::Command {
                entity_id: "e1".into(),
                verb: "turn-on".into(),
                brightness: Some(35.0),
                confirmation: Default::default(),
            }],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        execute(&plan, RunId::new(), &gateway).unwrap();
        assert_eq!(
            *gateway.0.lock().unwrap(),
            vec![
                ("e1".into(), "switch".into(), StateValue::Bool(true)),
                (
                    "e1".into(),
                    "light.brightness".into(),
                    StateValue::Percentage(35.0),
                ),
            ]
        );
    }

    #[test]
    fn reported_confirmation_suspends_on_the_exact_command_receipt() {
        let gateway = Fake(Mutex::new(Vec::new()));
        let plan = ExecutionPlan {
            actions: vec![PlannedAction::Command {
                entity_id: "e1".into(),
                verb: "turn-on".into(),
                brightness: None,
                confirmation: CommandConfirmation::Reported {
                    timeout_milliseconds: 5_000,
                },
            }],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        assert!(matches!(
            execute(&plan, RunId::new(), &gateway).unwrap(),
            RunResult::Suspended {
                after_milliseconds: Some(5_000),
                next_action: 1,
                awaiting: Some(AwaitTrigger::CommandConfirmed { command_id }),
                await_timeout_is_failure: true,
                ..
            } if command_id == "command-1"
        ));
    }

    struct Flaky(Mutex<u8>);
    impl CommandGateway for Flaky {
        fn request(
            &self,
            _entity: &str,
            _key: &str,
            _value: StateValue,
            _idempotency_key: String,
        ) -> Result<String, String> {
            let mut attempts = self.0.lock().unwrap();
            *attempts += 1;
            (*attempts > 1)
                .then_some("command-1".into())
                .ok_or_else(|| "bridge unavailable".into())
        }

        fn set_automation_enabled(
            &self,
            _flow_id: &str,
            _enabled: bool,
            _idempotency_key: String,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn retry_suspends_with_a_persistable_attempt_then_completes() {
        let gateway = Flaky(Mutex::new(0));
        let plan = ExecutionPlan {
            actions: vec![PlannedAction::Retry {
                action: Box::new(PlannedAction::Command {
                    entity_id: "e1".into(),
                    verb: "turn-on".into(),
                    brightness: None,
                    confirmation: Default::default(),
                }),
                attempts: 3,
                backoff_milliseconds: 200,
            }],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        let run_id = RunId::new();
        let first = execute(&plan, run_id.clone(), &gateway).unwrap();
        let RunResult::Suspended {
            after_milliseconds: Some(200),
            next_action: 0,
            retry_attempt: Some(1),
            trace,
            ..
        } = first
        else {
            panic!("first failure should schedule a retry");
        };
        let completed =
            execute_from_resumption(&plan, run_id, &gateway, 0, Some(1), trace).unwrap();
        assert!(matches!(
            completed,
            RunResult::Completed(RunTrace { steps })
                if matches!(steps.as_slice(), [
                    TraceStep::RetryScheduled { next_attempt: 2, total_attempts: 3, backoff_milliseconds: 200 },
                    TraceStep::CommandRequested { entity_id, verb },
                ] if entity_id == "e1" && verb == "turn-on")
        ));
        assert_eq!(*gateway.0.lock().unwrap(), 2);
    }

    #[test]
    fn retry_reports_a_terminal_failure_when_its_bound_is_exhausted() {
        let gateway = Flaky(Mutex::new(0));
        let plan = ExecutionPlan {
            actions: vec![PlannedAction::Retry {
                action: Box::new(PlannedAction::Command {
                    entity_id: "e1".into(),
                    verb: "turn-on".into(),
                    brightness: None,
                    confirmation: Default::default(),
                }),
                attempts: 1,
                backoff_milliseconds: 1,
            }],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        assert!(matches!(
            execute(&plan, RunId::new(), &gateway).unwrap(),
            RunResult::Failed(RunTrace { steps })
                if matches!(steps.as_slice(), [TraceStep::RetryExhausted { attempts: 1 }])
        ));
    }

    #[test]
    fn a_non_retryable_command_failure_is_terminal_and_traced() {
        let gateway = Flaky(Mutex::new(0));
        let plan = ExecutionPlan {
            actions: vec![PlannedAction::Command {
                entity_id: "e1".into(),
                verb: "turn-on".into(),
                brightness: None,
                confirmation: Default::default(),
            }],
            max_runtime_milliseconds: None,
            concurrency: Default::default(),
        };
        assert!(matches!(
            execute(&plan, RunId::new(), &gateway).unwrap(),
            RunResult::Failed(RunTrace { steps })
                if matches!(steps.as_slice(), [TraceStep::ActionFailed { action }] if action == "command turn-on failed")
        ));
    }
}
