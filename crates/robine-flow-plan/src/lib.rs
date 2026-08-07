//! Compilation d'un AST Flow validé vers un plan sans effet de bord.

use robine_flow_ast::{FlowAst, Form};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionPlan {
    pub actions: Vec<PlannedAction>,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlannedAction {
    Command { entity_id: String, verb: String },
    Wait { milliseconds: u64 },
    Audit { message: String },
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
    Ok(ExecutionPlan { actions })
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
            actions.push(PlannedAction::Command {
                entity_id: entity,
                verb,
            });
            Ok(())
        }
        Some("wait") => match items.get(1) {
            Some(Form::Number {
                literal,
                unit: Some(unit),
            }) if unit == "ms" => {
                actions.push(PlannedAction::Wait {
                    milliseconds: literal.parse().map_err(|_| PlanError::InvalidWait)?,
                });
                Ok(())
            }
            _ => Err(PlanError::InvalidWait),
        },
        Some("audit") => match items.get(2) {
            Some(Form::String(message)) => {
                actions.push(PlannedAction::Audit {
                    message: message.clone(),
                });
                Ok(())
            }
            _ => Err(PlanError::UnsupportedAction),
        },
        _ => Err(PlanError::UnsupportedAction),
    }
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
    #[error("wait must use milliseconds")]
    InvalidWait,
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
}
