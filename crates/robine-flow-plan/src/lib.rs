//! Compilation d'un AST Flow validé vers un plan sans effet de bord.

use robine_flow_ast::{FlowAst, Form};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionPlan {
    pub actions: Vec<PlannedAction>,
    #[serde(default)]
    pub max_runtime_milliseconds: Option<u64>,
    #[serde(default)]
    pub concurrency: ConcurrencyPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConcurrencyPolicy {
    pub mode: ConcurrencyMode,
    /// `queue` borne le nombre total d'exécutions actives ou en attente.
    pub max_runs: u8,
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        Self {
            mode: ConcurrencyMode::Single,
            max_runs: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyMode {
    Single,
    Restart,
    Queue,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlannedAction {
    Command {
        entity_id: String,
        verb: String,
        brightness: Option<f64>,
        #[serde(default)]
        confirmation: CommandConfirmation,
    },
    Wait {
        milliseconds: u64,
    },
    Await {
        trigger: AwaitTrigger,
        timeout_milliseconds: Option<u64>,
    },
    Audit {
        message: String,
    },
    SetAutomationEnabled {
        flow_id: String,
        enabled: bool,
    },
    /// Une seule action idempotente, réessayée au plus `attempts` fois. Le
    /// runtime persiste l'index de tentative avant chaque backoff.
    Retry {
        action: Box<PlannedAction>,
        attempts: u8,
        backoff_milliseconds: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandConfirmation {
    #[default]
    Transport,
    None,
    Reported {
        timeout_milliseconds: u64,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AwaitTrigger {
    AnyOf {
        triggers: Vec<AwaitTrigger>,
    },
    EventType {
        event_type: String,
    },
    /// Confirmation d'une commande Robine précise. Le plan ne contient jamais
    /// cette forme : elle est créée à l'exécution avec le `command_id` retourné
    /// par le port de commande, puis persiste dans le point de reprise.
    CommandConfirmed {
        command_id: String,
    },
    StateChanged {
        entity_id: String,
        property: String,
        to: Option<AwaitValue>,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AwaitValue {
    Bool(bool),
    Percentage(f64),
    Text(String),
}

pub fn compile(ast: &FlowAst) -> Result<ExecutionPlan, PlanError> {
    ast.validate()
        .map_err(|error| PlanError::InvalidFlow(error.to_string()))?;
    let body = ast
        .root
        .list()
        .and_then(|forms| {
            forms.iter().find(|form| {
                form.list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("do")
            })
        })
        .and_then(Form::list)
        .ok_or(PlanError::MissingBody)?;
    let mut actions = Vec::new();
    for form in &body[1..] {
        compile_action(form, &mut actions)?;
    }
    Ok(ExecutionPlan {
        actions,
        max_runtime_milliseconds: flow_max_runtime(&ast.root)?,
        concurrency: flow_concurrency(&ast.root)?,
    })
}

fn flow_concurrency(root: &Form) -> Result<ConcurrencyPolicy, PlanError> {
    let Some(meta) = root
        .list()
        .and_then(|forms| {
            forms.iter().find(|form| {
                form.list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("meta")
            })
        })
        .and_then(Form::list)
    else {
        return Ok(ConcurrencyPolicy::default());
    };
    let mut policy = ConcurrencyPolicy::default();
    for pair in meta[1..].chunks(2) {
        match pair {
            [Form::Keyword(key), Form::Keyword(value)] if key == "mode" => {
                policy.mode = match value.as_str() {
                    "single" => ConcurrencyMode::Single,
                    "restart" => ConcurrencyMode::Restart,
                    "queue" => ConcurrencyMode::Queue,
                    _ => return Err(PlanError::InvalidConcurrency),
                };
            }
            [
                Form::Keyword(key),
                Form::Number {
                    literal,
                    unit: None,
                },
            ] if key == "max-runs" => {
                policy.max_runs = literal
                    .parse::<u8>()
                    .ok()
                    .filter(|value| (1..=32).contains(value))
                    .ok_or(PlanError::InvalidConcurrency)?;
            }
            _ => {}
        }
    }
    Ok(policy)
}

fn flow_max_runtime(root: &Form) -> Result<Option<u64>, PlanError> {
    let Some(meta) = root
        .list()
        .and_then(|forms| {
            forms.iter().find(|form| {
                form.list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("meta")
            })
        })
        .and_then(Form::list)
    else {
        return Ok(None);
    };
    for pair in meta[1..].chunks(2) {
        if matches!(pair.first(), Some(Form::Keyword(key)) if key == "max-runtime") {
            if let [
                _,
                Form::Number {
                    literal,
                    unit: Some(unit),
                },
            ] = pair
            {
                return duration_milliseconds(literal, unit).map(Some);
            }
            return Err(PlanError::InvalidWait);
        }
    }
    Ok(None)
}
fn compile_action(form: &Form, actions: &mut Vec<PlannedAction>) -> Result<(), PlanError> {
    let items = form.list().ok_or(PlanError::UnsupportedAction)?;
    match items.first().and_then(Form::symbol) {
        Some("command") => {
            let entity = items
                .get(1)
                .and_then(Form::list)
                .and_then(|entity| entity.get(1))
                .and_then(|value| match value {
                    Form::String(value) => Some(value.clone()),
                    _ => None,
                })
                .ok_or(PlanError::InvalidCommand)?;
            let verb = items
                .get(2)
                .and_then(|value| match value {
                    Form::Keyword(value) => Some(value.clone()),
                    _ => None,
                })
                .ok_or(PlanError::InvalidCommand)?;
            let options = command_options(items)?;
            actions.push(PlannedAction::Command {
                entity_id: entity,
                verb,
                brightness: options.brightness,
                confirmation: options.confirmation,
            });
            Ok(())
        }
        Some("wait") => match items.get(1) {
            Some(Form::Number {
                literal,
                unit: Some(unit),
            }) => {
                actions.push(PlannedAction::Wait {
                    milliseconds: duration_milliseconds(literal, unit)?,
                });
                Ok(())
            }
            _ => Err(PlanError::InvalidWait),
        },
        Some("await") => {
            let trigger = items
                .get(1)
                .and_then(Form::list)
                .ok_or(PlanError::InvalidAwait)
                .and_then(compile_await_trigger)?;
            let timeout_milliseconds = match items.get(2..) {
                Some([]) => None,
                Some(
                    [
                        Form::Keyword(key),
                        Form::Number {
                            literal,
                            unit: Some(unit),
                        },
                    ],
                ) if key == "timeout" => Some(duration_milliseconds(literal, unit)?),
                _ => return Err(PlanError::InvalidAwait),
            };
            actions.push(PlannedAction::Await {
                trigger,
                timeout_milliseconds,
            });
            Ok(())
        }
        Some("audit") => match items.get(2) {
            Some(Form::String(message)) => {
                actions.push(PlannedAction::Audit {
                    message: message.clone(),
                });
                Ok(())
            }
            _ => Err(PlanError::UnsupportedAction),
        },
        Some("activate" | "deactivate") if items.len() == 2 => {
            let flow_id = items
                .get(1)
                .and_then(Form::list)
                .filter(|target| target.first().and_then(Form::symbol) == Some("flow"))
                .and_then(|target| target.get(1))
                .and_then(|target| match target {
                    Form::String(value) if !value.is_empty() => Some(value.clone()),
                    _ => None,
                })
                .ok_or(PlanError::InvalidAutomationTarget)?;
            actions.push(PlannedAction::SetAutomationEnabled {
                flow_id,
                enabled: items.first().and_then(Form::symbol) == Some("activate"),
            });
            Ok(())
        }
        Some("retry") => {
            let action = items.get(1).ok_or(PlanError::InvalidRetry)?;
            let mut nested = Vec::new();
            compile_action(action, &mut nested)?;
            let action = nested
                .pop()
                .filter(is_retryable)
                .ok_or(PlanError::InvalidRetry)?;
            if !nested.is_empty() {
                return Err(PlanError::InvalidRetry);
            }
            let (attempts, backoff_milliseconds) = match items.get(2..) {
                Some(
                    [
                        Form::Keyword(times),
                        Form::Number {
                            literal,
                            unit: None,
                        },
                        Form::Keyword(backoff),
                        Form::Number {
                            literal: backoff_literal,
                            unit: Some(backoff_unit),
                        },
                    ],
                ) if times == "times" && backoff == "backoff" => {
                    let attempts = literal
                        .parse::<u8>()
                        .ok()
                        .filter(|attempts| (1..=10).contains(attempts))
                        .ok_or(PlanError::InvalidRetry)?;
                    (
                        attempts,
                        duration_milliseconds(backoff_literal, backoff_unit)?,
                    )
                }
                _ => return Err(PlanError::InvalidRetry),
            };
            actions.push(PlannedAction::Retry {
                action: Box::new(action),
                attempts,
                backoff_milliseconds,
            });
            Ok(())
        }
        _ => Err(PlanError::UnsupportedAction),
    }
}

fn is_retryable(action: &PlannedAction) -> bool {
    matches!(
        action,
        PlannedAction::Command { .. }
            | PlannedAction::Audit { .. }
            | PlannedAction::SetAutomationEnabled { .. }
    )
}

fn compile_await_trigger(trigger: &[Form]) -> Result<AwaitTrigger, PlanError> {
    match trigger.first().and_then(Form::symbol) {
        Some("any-of") => {
            let triggers = trigger[1..]
                .iter()
                .map(Form::list)
                .collect::<Option<Vec<_>>>()
                .ok_or(PlanError::InvalidAwait)?
                .into_iter()
                .map(compile_await_trigger)
                .collect::<Result<Vec<_>, _>>()?;
            if triggers.is_empty() {
                return Err(PlanError::InvalidAwait);
            }
            Ok(AwaitTrigger::AnyOf { triggers })
        }
        Some("event") => {
            let event_type = trigger
                .windows(2)
                .find_map(|pair| {
                    matches!(&pair[0], Form::Keyword(key) if key == "type")
                        .then(|| match &pair[1] {
                            Form::String(value) | Form::Symbol(value) => Some(value.clone()),
                            _ => None,
                        })
                        .flatten()
                })
                .filter(|event_type| !event_type.is_empty())
                .ok_or(PlanError::InvalidAwait)?;
            Ok(AwaitTrigger::EventType { event_type })
        }
        Some("state-changed") => {
            let entity_id = trigger
                .get(1)
                .and_then(Form::list)
                .and_then(|entity| entity.get(1))
                .and_then(|value| match value {
                    Form::String(value) => Some(value.clone()),
                    _ => None,
                })
                .filter(|entity_id| !entity_id.is_empty())
                .ok_or(PlanError::InvalidAwait)?;
            let mut property = None;
            let mut to = None;
            let mut index = 2;
            while index + 1 < trigger.len() {
                let Form::Keyword(key) = &trigger[index] else {
                    return Err(PlanError::InvalidAwait);
                };
                match key.as_str() {
                    "property" => {
                        property = match &trigger[index + 1] {
                            Form::String(value) if !value.is_empty() => Some(value.clone()),
                            _ => return Err(PlanError::InvalidAwait),
                        };
                    }
                    "to" => {
                        to = Some(match &trigger[index + 1] {
                            Form::Bool(value) => AwaitValue::Bool(*value),
                            Form::Number {
                                literal,
                                unit: Some(unit),
                            } if unit == "%" => AwaitValue::Percentage(
                                literal.parse().map_err(|_| PlanError::InvalidAwait)?,
                            ),
                            Form::String(value) => AwaitValue::Text(value.clone()),
                            _ => return Err(PlanError::InvalidAwait),
                        });
                    }
                    _ => return Err(PlanError::InvalidAwait),
                }
                index += 2;
            }
            Ok(AwaitTrigger::StateChanged {
                entity_id,
                property: property.ok_or(PlanError::InvalidAwait)?,
                to,
            })
        }
        _ => Err(PlanError::InvalidAwait),
    }
}

const MAX_WAIT_MILLISECONDS: u64 = 30 * 24 * 60 * 60 * 1_000;

fn duration_milliseconds(literal: &str, unit: &str) -> Result<u64, PlanError> {
    let value = literal.parse::<f64>().map_err(|_| PlanError::InvalidWait)?;
    let multiplier = match unit {
        "ms" => 1.0,
        "s" => 1_000.0,
        "m" => 60_000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        _ => return Err(PlanError::InvalidWait),
    };
    let milliseconds = value * multiplier;
    if !milliseconds.is_finite()
        || milliseconds <= 0.0
        || milliseconds.fract() != 0.0
        || milliseconds > MAX_WAIT_MILLISECONDS as f64
    {
        return Err(PlanError::InvalidWait);
    }
    Ok(milliseconds as u64)
}

struct CommandOptions {
    brightness: Option<f64>,
    confirmation: CommandConfirmation,
}

fn command_options(items: &[Form]) -> Result<CommandOptions, PlanError> {
    let mut brightness = None;
    let mut confirmation = None;
    let mut timeout = None;
    let mut index = 3;
    while index + 1 < items.len() {
        let Form::Keyword(key) = &items[index] else {
            return Err(PlanError::InvalidCommand);
        };
        match key.as_str() {
            "brightness" if brightness.is_none() => {
                brightness = match &items[index + 1] {
                    Form::Number {
                        literal,
                        unit: Some(unit),
                    } if unit == "%" => literal
                        .parse::<f64>()
                        .ok()
                        .filter(|value| (0.0..=100.0).contains(value))
                        .ok_or(PlanError::InvalidBrightness)
                        .map(Some)?,
                    _ => return Err(PlanError::InvalidBrightness),
                };
            }
            "confirm" if confirmation.is_none() => {
                confirmation = Some(match &items[index + 1] {
                    Form::Keyword(value) if value == "transport" => CommandConfirmation::Transport,
                    Form::Keyword(value) if value == "none" => CommandConfirmation::None,
                    Form::Keyword(value) if value == "reported" => CommandConfirmation::Reported {
                        timeout_milliseconds: 0,
                    },
                    _ => return Err(PlanError::InvalidConfirmation),
                });
            }
            "timeout" if timeout.is_none() => {
                timeout = match &items[index + 1] {
                    Form::Number {
                        literal,
                        unit: Some(unit),
                    } => Some(duration_milliseconds(literal, unit)?),
                    _ => return Err(PlanError::InvalidConfirmation),
                };
            }
            _ => return Err(PlanError::InvalidCommand),
        }
        index += 2;
    }
    if index != items.len() {
        return Err(PlanError::InvalidCommand);
    }
    let confirmation = match confirmation.unwrap_or_default() {
        CommandConfirmation::Reported { .. } => {
            if brightness.is_some() {
                return Err(PlanError::InvalidConfirmation);
            }
            CommandConfirmation::Reported {
                timeout_milliseconds: timeout.ok_or(PlanError::InvalidConfirmation)?,
            }
        }
        _ if timeout.is_some() => return Err(PlanError::InvalidConfirmation),
        confirmation => confirmation,
    };
    Ok(CommandOptions {
        brightness,
        confirmation,
    })
}
#[derive(Debug, Error)]
pub enum PlanError {
    #[error("invalid Flow: {0}")]
    InvalidFlow(String),
    #[error("missing Flow body")]
    MissingBody,
    #[error("unsupported Flow action")]
    UnsupportedAction,
    #[error("invalid command action")]
    InvalidCommand,
    #[error("wait must use a positive, integral duration no longer than thirty days")]
    InvalidWait,
    #[error("invalid Flow concurrency policy")]
    InvalidConcurrency,
    #[error("activate/deactivate requires an explicit Flow reference")]
    InvalidAutomationTarget,
    #[error("brightness must be a percentage between 0 and 100")]
    InvalidBrightness,
    #[error(
        "reported confirmation requires :confirm :reported and a positive :timeout, without :brightness"
    )]
    InvalidConfirmation,
    #[error("await must use an event trigger with :type and an optional bounded :timeout")]
    InvalidAwait,
    #[error("retry requires one idempotent action, :times 1..10 and a positive :backoff duration")]
    InvalidRetry,
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_flow_ast::Form;

    #[test]
    fn compiles_a_bounded_command_sequence() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("command".into()),
                    Form::List(vec![
                        Form::Symbol("entity".into()),
                        Form::String("ent_light".into()),
                    ]),
                    Form::Keyword("turn-on".into()),
                ]),
                Form::List(vec![
                    Form::Symbol("wait".into()),
                    Form::Number {
                        literal: "500".into(),
                        unit: Some("ms".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert_eq!(compile(&flow).unwrap().actions.len(), 2);
    }

    #[test]
    fn preserves_brightness_in_the_execution_plan() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("command".into()),
                    Form::List(vec![
                        Form::Symbol("entity".into()),
                        Form::String("ent_light".into()),
                    ]),
                    Form::Keyword("turn-on".into()),
                    Form::Keyword("brightness".into()),
                    Form::Number {
                        literal: "42".into(),
                        unit: Some("%".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Command { brightness: Some(value), .. }] if (*value - 42.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn compiles_a_bounded_reported_confirmation() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("command".into()),
                    Form::List(vec![
                        Form::Symbol("entity".into()),
                        Form::String("ent_light".into()),
                    ]),
                    Form::Keyword("turn-on".into()),
                    Form::Keyword("confirm".into()),
                    Form::Keyword("reported".into()),
                    Form::Keyword("timeout".into()),
                    Form::Number {
                        literal: "5".into(),
                        unit: Some("s".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Command {
                confirmation: CommandConfirmation::Reported {
                    timeout_milliseconds: 5_000
                },
                ..
            }]
        ));
    }

    #[test]
    fn compiles_all_documented_wait_units_into_a_persistable_duration() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("wait".into()),
                    Form::Number {
                        literal: "2".into(),
                        unit: Some("m".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Wait {
                milliseconds: 120_000
            }]
        ));
        assert!(duration_milliseconds("0", "s").is_err());
        assert!(duration_milliseconds("31", "d").is_err());
    }

    #[test]
    fn compiles_an_event_await_with_a_persistable_timeout() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("await".into()),
                    Form::List(vec![
                        Form::Symbol("event".into()),
                        Form::Keyword("type".into()),
                        Form::String("door.closed".into()),
                    ]),
                    Form::Keyword("timeout".into()),
                    Form::Number {
                        literal: "5".into(),
                        unit: Some("m".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Await { trigger: AwaitTrigger::EventType { event_type }, timeout_milliseconds: Some(300_000) }] if event_type == "door.closed"
        ));
    }

    #[test]
    fn compiles_a_state_await_with_a_boolean_expected_value() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("await".into()),
                    Form::List(vec![
                        Form::Symbol("state-changed".into()),
                        Form::List(vec![
                            Form::Symbol("entity".into()),
                            Form::String("00000000-0000-0000-0000-000000000001".into()),
                        ]),
                        Form::Keyword("property".into()),
                        Form::String("switch".into()),
                        Form::Keyword("to".into()),
                        Form::Bool(true),
                    ]),
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Await { trigger: AwaitTrigger::StateChanged { property, to: Some(AwaitValue::Bool(true)), .. }, .. }] if property == "switch"
        ));
    }

    #[test]
    fn compiles_an_any_of_await_with_event_and_state_branches() {
        let entity = "00000000-0000-0000-0000-000000000001";
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("await".into()),
                    Form::List(vec![
                        Form::Symbol("any-of".into()),
                        Form::List(vec![
                            Form::Symbol("event".into()),
                            Form::Keyword("type".into()),
                            Form::String("door.closed".into()),
                        ]),
                        Form::List(vec![
                            Form::Symbol("state-changed".into()),
                            Form::List(vec![
                                Form::Symbol("entity".into()),
                                Form::String(entity.into()),
                            ]),
                            Form::Keyword("property".into()),
                            Form::String("switch".into()),
                            Form::Keyword("to".into()),
                            Form::Bool(false),
                        ]),
                    ]),
                ]),
            ]),
        ]))
        .unwrap();

        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Await { trigger: AwaitTrigger::AnyOf { triggers }, .. }]
                if matches!(triggers.as_slice(), [AwaitTrigger::EventType { event_type }, AwaitTrigger::StateChanged { .. }] if event_type == "door.closed")
        ));
    }

    #[test]
    fn compiles_a_bounded_retry_of_an_idempotent_command() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![
                Form::Symbol("do".into()),
                Form::List(vec![
                    Form::Symbol("retry".into()),
                    Form::List(vec![
                        Form::Symbol("command".into()),
                        Form::List(vec![
                            Form::Symbol("entity".into()),
                            Form::String("ent_light".into()),
                        ]),
                        Form::Keyword("turn-on".into()),
                    ]),
                    Form::Keyword("times".into()),
                    Form::Number {
                        literal: "3".into(),
                        unit: None,
                    },
                    Form::Keyword("backoff".into()),
                    Form::Number {
                        literal: "2".into(),
                        unit: Some("s".into()),
                    },
                ]),
            ]),
        ]))
        .unwrap();
        assert!(matches!(
            compile(&flow).unwrap().actions.as_slice(),
            [PlannedAction::Retry { action, attempts: 3, backoff_milliseconds: 2_000 }]
                if matches!(action.as_ref(), PlannedAction::Command { entity_id, verb, .. } if entity_id == "ent_light" && verb == "turn-on")
        ));
    }

    #[test]
    fn rejects_unbounded_or_suspending_retry_actions() {
        let invalid = |action| {
            FlowAst::new(Form::List(vec![
                Form::Symbol("flow".into()),
                Form::List(vec![
                    Form::Symbol("on".into()),
                    Form::List(vec![Form::Symbol("event".into())]),
                ]),
                Form::List(vec![
                    Form::Symbol("do".into()),
                    Form::List(vec![
                        Form::Symbol("retry".into()),
                        action,
                        Form::Keyword("times".into()),
                        Form::Number {
                            literal: "11".into(),
                            unit: None,
                        },
                        Form::Keyword("backoff".into()),
                        Form::Number {
                            literal: "1".into(),
                            unit: Some("s".into()),
                        },
                    ]),
                ]),
            ]))
            .unwrap()
        };
        assert!(matches!(
            compile(&invalid(Form::List(vec![
                Form::Symbol("wait".into()),
                Form::Number {
                    literal: "1".into(),
                    unit: Some("s".into())
                },
            ]))),
            Err(PlanError::InvalidRetry)
        ));
    }
}
