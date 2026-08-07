//! Vérification structurelle et sémantique de Robine Flow, sans I/O.

use robine_flow_ast::{AstError, FlowAst, Form};
use std::collections::HashSet;

pub trait CapabilityCatalog {
    fn capabilities_for(&self, entity_id: &str) -> Option<HashSet<String>>;
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CheckDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub path: String,
}

pub fn validate(ast: &FlowAst, catalog: &impl CapabilityCatalog) -> Vec<CheckDiagnostic> {
    if let Err(error) = ast.validate() {
        return vec![root_error(error)];
    }
    let mut diagnostics = Vec::new();
    visit(&ast.root, "$", catalog, &mut diagnostics);
    diagnostics
}

fn visit(
    form: &Form,
    path: &str,
    catalog: &impl CapabilityCatalog,
    diagnostics: &mut Vec<CheckDiagnostic>,
) {
    let Some(items) = form.list() else {
        return;
    };
    let head = items.first().and_then(Form::symbol);
    if head == Some("command") {
        validate_command(items, path, catalog, diagnostics);
    }
    if matches!(head, Some("<" | "<=" | ">" | ">=")) {
        validate_comparison(items, path, diagnostics);
    }
    for (index, child) in items.iter().enumerate() {
        visit(child, &format!("{path}/{index}"), catalog, diagnostics);
    }
}

fn validate_command(
    items: &[Form],
    path: &str,
    catalog: &impl CapabilityCatalog,
    diagnostics: &mut Vec<CheckDiagnostic>,
) {
    let Some(entity_id) = entity_id(items.get(1)) else {
        diagnostics.push(diag(
            "flow.command_target",
            "command requires an explicit entity reference",
            path,
        ));
        return;
    };
    let Some(capabilities) = catalog.capabilities_for(entity_id) else {
        diagnostics.push(diag(
            "flow.unknown_entity",
            "referenced entity does not exist",
            path,
        ));
        return;
    };
    let verb = match items.get(2) {
        Some(Form::Keyword(value)) => Some(value.as_str()),
        _ => None,
    };
    let required = match verb {
        Some("turn-on") | Some("turn-off") => Some("switch"),
        _ => None,
    };
    if let Some(required) = required {
        if !capabilities.contains(required) {
            diagnostics.push(diag(
                "flow.command_unsupported",
                &format!("entity does not support {required}"),
                path,
            ));
        }
    } else {
        diagnostics.push(diag("flow.command_verb", "unsupported command verb", path));
    }
    let mut index = 3;
    while index + 1 < items.len() {
        if matches!(items[index], Form::Keyword(ref key) if key == "brightness") {
            if !capabilities.contains("light.brightness") {
                diagnostics.push(diag(
                    "flow.command_unsupported",
                    "entity does not support light.brightness",
                    path,
                ));
            }
            if !matches!(items[index + 1], Form::Number { ref unit, .. } if unit.as_deref() == Some("%"))
            {
                diagnostics.push(diag(
                    "flow.brightness_unit",
                    "brightness must use a percentage",
                    path,
                ));
            }
        }
        index += 2;
    }
}

fn validate_comparison(items: &[Form], path: &str, diagnostics: &mut Vec<CheckDiagnostic>) {
    if items.len() != 3 {
        diagnostics.push(diag(
            "flow.comparison_arity",
            "comparison requires exactly two operands",
            path,
        ));
        return;
    }
    if let (Form::Number { unit: left, .. }, Form::Number { unit: right, .. }) =
        (&items[1], &items[2])
    {
        if left != right {
            diagnostics.push(diag(
                "flow.unit_mismatch",
                "comparison operands require compatible units",
                path,
            ));
        }
    }
}

fn entity_id(form: Option<&Form>) -> Option<&str> {
    let items = form?.list()?;
    (items.first().and_then(Form::symbol) == Some("entity")).then_some(())?;
    match items.get(1)? {
        Form::String(value) => Some(value),
        _ => None,
    }
}

fn diag(code: &'static str, message: &str, path: &str) -> CheckDiagnostic {
    CheckDiagnostic {
        code,
        message: message.into(),
        path: path.into(),
    }
}
fn root_error(error: AstError) -> CheckDiagnostic {
    diag("flow.root", &error.to_string(), "$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_flow_ast::Form;
    use std::collections::HashMap;

    struct Catalog(HashMap<String, HashSet<String>>);
    impl CapabilityCatalog for Catalog {
        fn capabilities_for(&self, entity_id: &str) -> Option<HashSet<String>> {
            self.0.get(entity_id).cloned()
        }
    }
    fn ast(body: Form) -> FlowAst {
        FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![Form::Symbol("do".into()), body]),
        ]))
        .unwrap()
    }

    #[test]
    fn validates_a_supported_light_command() {
        let catalog = Catalog(HashMap::from([(
            "ent_light".into(),
            HashSet::from(["switch".into(), "light.brightness".into()]),
        )]));
        let flow = ast(Form::List(vec![
            Form::Symbol("command".into()),
            Form::List(vec![
                Form::Symbol("entity".into()),
                Form::String("ent_light".into()),
            ]),
            Form::Keyword("turn-on".into()),
            Form::Keyword("brightness".into()),
            Form::Number {
                literal: "40".into(),
                unit: Some("%".into()),
            },
        ]));
        assert!(validate(&flow, &catalog).is_empty());
    }

    #[test]
    fn rejects_mixed_dimension_comparison() {
        let catalog = Catalog(HashMap::new());
        let flow = ast(Form::List(vec![
            Form::Symbol("choose".into()),
            Form::List(vec![
                Form::Symbol("<".into()),
                Form::Number {
                    literal: "20".into(),
                    unit: None,
                },
                Form::Number {
                    literal: "21".into(),
                    unit: Some("%".into()),
                },
            ]),
        ]));
        assert!(
            validate(&flow, &catalog)
                .iter()
                .any(|diagnostic| diagnostic.code == "flow.unit_mismatch")
        );
    }
}
