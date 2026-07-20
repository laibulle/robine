use crate::{Diagnostic, Expr, ExprKind, Function, Program, Span, StableId, StmtKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Read,
    Resolved,
    Typed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Unit,
    Bool,
    Int,
    Text,
    Console,
    Unknown(String),
}

impl Type {
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "Unit" => Self::Unit,
            "Bool" => Self::Bool,
            "Int" => Self::Int,
            "Text" => Self::Text,
            "Console" => Self::Console,
            other => Self::Unknown(other.to_owned()),
        }
    }

    #[must_use]
    pub const fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => formatter.write_str("Unit"),
            Self::Bool => formatter.write_str("Bool"),
            Self::Int => formatter.write_str("Int"),
            Self::Text => formatter.write_str("Text"),
            Self::Console => formatter.write_str("Console"),
            Self::Unknown(name) => formatter.write_str(name),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Module,
    Function,
    Parameter,
    Local,
    Type,
    Effect,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub id: StableId,
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub definition_span: Span,
    pub type_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionSummary {
    pub id: StableId,
    pub name: String,
    pub span: Span,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub declared_effects: BTreeSet<String>,
    pub required_effects: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub phase: Phase,
    pub program: Option<Program>,
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<SymbolInfo>,
    pub functions: Vec<FunctionSummary>,
    pub foreign_calls: BTreeSet<String>,
}

impl Analysis {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty() && self.phase == Phase::Typed
    }

    #[must_use]
    pub fn symbol_at(&self, offset: usize) -> Option<&SymbolInfo> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.span.contains(offset))
            .min_by_key(|symbol| symbol.span.end.saturating_sub(symbol.span.start))
    }
}

#[must_use]
pub fn analyze(source: &str) -> Analysis {
    analyze_incremental(source, None).0
}

pub(crate) fn analyze_incremental(
    source: &str,
    old_tree: Option<&tree_sitter::Tree>,
) -> (Analysis, Option<tree_sitter::Tree>) {
    let (parsed, tree) = crate::syntax::parse_incremental(source, old_tree);
    let program = match parsed {
        Ok(program) => program,
        Err(diagnostics) => {
            return (
                Analysis {
                    phase: Phase::Read,
                    program: None,
                    diagnostics,
                    symbols: Vec::new(),
                    functions: Vec::new(),
                    foreign_calls: BTreeSet::new(),
                },
                tree,
            );
        }
    };
    (Analyzer::new(program).run(), tree)
}

struct Analyzer {
    program: Program,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<SymbolInfo>,
    signatures: BTreeMap<String, FunctionSummary>,
    foreign_calls: BTreeSet<String>,
}

impl Analyzer {
    fn new(program: Program) -> Self {
        Self {
            program,
            diagnostics: Vec::new(),
            symbols: Vec::new(),
            signatures: BTreeMap::new(),
            foreign_calls: BTreeSet::new(),
        }
    }

    fn run(mut self) -> Analysis {
        self.symbols.push(SymbolInfo {
            id: StableId::named("module", &self.program.module),
            name: self.program.module.clone(),
            kind: SymbolKind::Module,
            span: self.program.module_span,
            definition_span: self.program.module_span,
            type_name: None,
        });

        for function in &self.program.functions {
            if self.signatures.contains_key(&function.name) {
                self.diagnostics.push(Diagnostic::error(
                    "RBN2001",
                    format!("fonction `{}` définie plusieurs fois", function.name),
                    function.name_span,
                ));
                continue;
            }
            let params = function
                .params
                .iter()
                .map(|param| (param.name.clone(), Type::from_name(&param.type_name)))
                .collect();
            let summary = FunctionSummary {
                id: function.id,
                name: function.name.clone(),
                span: function.name_span,
                params,
                return_type: Type::from_name(&function.return_type),
                declared_effects: function
                    .effects
                    .iter()
                    .map(|(effect, _)| effect.clone())
                    .collect(),
                required_effects: BTreeSet::new(),
            };
            self.signatures.insert(function.name.clone(), summary);
        }

        let functions = self.program.functions.clone();
        for function in &functions {
            self.check_function(function);
        }

        Analysis {
            phase: Phase::Typed,
            program: Some(self.program),
            diagnostics: self.diagnostics,
            symbols: self.symbols,
            functions: self.signatures.into_values().collect(),
            foreign_calls: self.foreign_calls,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn check_function(&mut self, function: &Function) {
        let Some(signature) = self.signatures.get(&function.name).cloned() else {
            return;
        };
        self.symbols.push(SymbolInfo {
            id: function.id,
            name: function.name.clone(),
            kind: SymbolKind::Function,
            span: function.name_span,
            definition_span: function.name_span,
            type_name: Some(format_signature(&signature)),
        });

        if !signature.return_type.is_known() {
            self.diagnostics.push(Diagnostic::error(
                "RBN3000",
                format!("type inconnu `{}`", function.return_type),
                function.return_type_span,
            ));
        }

        for (effect, span) in &function.effects {
            self.symbols.push(SymbolInfo {
                id: StableId::named("effect", effect),
                name: effect.clone(),
                kind: SymbolKind::Effect,
                span: *span,
                definition_span: *span,
                type_name: Some("Effect".to_owned()),
            });
        }

        let mut environment = BTreeMap::new();
        for parameter in &function.params {
            let parameter_type = Type::from_name(&parameter.type_name);
            if !parameter_type.is_known() {
                self.diagnostics.push(Diagnostic::error(
                    "RBN3000",
                    format!("type inconnu `{}`", parameter.type_name),
                    parameter.type_span,
                ));
            }
            if environment
                .insert(
                    parameter.name.clone(),
                    (parameter_type.clone(), parameter.id, parameter.name_span),
                )
                .is_some()
            {
                self.diagnostics.push(Diagnostic::error(
                    "RBN2002",
                    format!("paramètre `{}` défini plusieurs fois", parameter.name),
                    parameter.name_span,
                ));
            }
            self.symbols.push(SymbolInfo {
                id: parameter.id,
                name: parameter.name.clone(),
                kind: SymbolKind::Parameter,
                span: parameter.name_span,
                definition_span: parameter.name_span,
                type_name: Some(parameter_type.to_string()),
            });
            self.symbols.push(SymbolInfo {
                id: StableId::named("type", &parameter.type_name),
                name: parameter.type_name.clone(),
                kind: SymbolKind::Type,
                span: parameter.type_span,
                definition_span: parameter.type_span,
                type_name: Some("Type".to_owned()),
            });
        }

        let mut required_effects = BTreeSet::new();
        let mut body_type = Type::Unit;
        for statement in &function.body {
            match &statement.kind {
                StmtKind::Let {
                    id,
                    name,
                    name_span,
                    annotation,
                    value,
                } => {
                    let inferred =
                        self.infer_expression(value, &environment, &mut required_effects);
                    let local_type = if let Some((type_name, type_span)) = annotation {
                        let annotated = Type::from_name(type_name);
                        if !annotated.is_known() {
                            self.diagnostics.push(Diagnostic::error(
                                "RBN3000",
                                format!("type inconnu `{type_name}`"),
                                *type_span,
                            ));
                        } else if inferred != annotated {
                            self.diagnostics.push(Diagnostic::error(
                                "RBN3001",
                                format!(
                                    "le binding `{name}` attend `{annotated}`, reçu `{inferred}`"
                                ),
                                value.span,
                            ));
                        }
                        annotated
                    } else {
                        inferred
                    };
                    environment.insert(name.clone(), (local_type.clone(), *id, *name_span));
                    self.symbols.push(SymbolInfo {
                        id: *id,
                        name: name.clone(),
                        kind: SymbolKind::Local,
                        span: *name_span,
                        definition_span: *name_span,
                        type_name: Some(local_type.to_string()),
                    });
                    body_type = Type::Unit;
                }
                StmtKind::Expr { value, terminated } => {
                    let inferred =
                        self.infer_expression(value, &environment, &mut required_effects);
                    body_type = if *terminated { Type::Unit } else { inferred };
                }
            }
        }

        if body_type != signature.return_type {
            self.diagnostics.push(Diagnostic::error(
                "RBN3002",
                format!(
                    "la fonction `{}` retourne `{body_type}`, signature `{}`",
                    function.name, signature.return_type
                ),
                function.span,
            ));
        }

        for effect in required_effects.difference(&signature.declared_effects) {
            self.diagnostics.push(Diagnostic::error(
                "RBN4001",
                format!(
                    "la fonction `{}` utilise l’effet `{effect}` sans le déclarer",
                    function.name
                ),
                function.name_span,
            ));
        }
        if let Some(summary) = self.signatures.get_mut(&function.name) {
            summary.required_effects = required_effects;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn infer_expression(
        &mut self,
        expression: &Expr,
        environment: &BTreeMap<String, (Type, StableId, Span)>,
        effects: &mut BTreeSet<String>,
    ) -> Type {
        match &expression.kind {
            ExprKind::Text(_) => Type::Text,
            ExprKind::Int(_) => Type::Int,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Var(name) => {
                if let Some((type_name, id, definition_span)) = environment.get(name) {
                    self.symbols.push(SymbolInfo {
                        id: *id,
                        name: name.clone(),
                        kind: SymbolKind::Local,
                        span: expression.span,
                        definition_span: *definition_span,
                        type_name: Some(type_name.to_string()),
                    });
                    type_name.clone()
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN2003",
                        format!("nom inconnu `{name}`"),
                        expression.span,
                    ));
                    Type::Unknown("<error>".to_owned())
                }
            }
            ExprKind::Call { path, args } => {
                if path.len() == 2 && path[0].0 == "rust" && path[1].0 == "grapheme_count" {
                    self.foreign_calls.insert("rust.grapheme_count".to_owned());
                    if args.len() == 1 {
                        let argument_type = self.infer_expression(&args[0], environment, effects);
                        if argument_type != Type::Text {
                            self.diagnostics.push(Diagnostic::error(
                                "RBN3001",
                                format!(
                                    "`rust.grapheme_count` attend `Text`, reçu `{argument_type}`"
                                ),
                                args[0].span,
                            ));
                        }
                    } else {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN3004",
                            "`rust.grapheme_count` attend exactement un argument",
                            expression.span,
                        ));
                    }
                    Type::Int
                } else if path.len() == 2 && path[1].0 == "write_line" {
                    let receiver = &path[0];
                    let Some((receiver_type, receiver_id, definition_span)) =
                        environment.get(&receiver.0)
                    else {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN2003",
                            format!("nom inconnu `{}`", receiver.0),
                            receiver.1,
                        ));
                        return Type::Unknown("<error>".to_owned());
                    };
                    self.symbols.push(SymbolInfo {
                        id: *receiver_id,
                        name: receiver.0.clone(),
                        kind: SymbolKind::Parameter,
                        span: receiver.1,
                        definition_span: *definition_span,
                        type_name: Some(receiver_type.to_string()),
                    });
                    if receiver_type != &Type::Console {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN3003",
                            format!("`write_line` exige `Console`, reçu `{receiver_type}`"),
                            receiver.1,
                        ));
                    }
                    if args.len() == 1 {
                        let argument_type = self.infer_expression(&args[0], environment, effects);
                        if argument_type != Type::Text {
                            self.diagnostics.push(Diagnostic::error(
                                "RBN3001",
                                format!("`write_line` attend `Text`, reçu `{argument_type}`"),
                                args[0].span,
                            ));
                        }
                    } else {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN3004",
                            "`write_line` attend exactement un argument",
                            expression.span,
                        ));
                    }
                    effects.insert("Console.Write".to_owned());
                    Type::Unit
                } else if path.len() == 1 {
                    let function_name = &path[0].0;
                    let Some(callee) = self.signatures.get(function_name).cloned() else {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN2004",
                            format!("fonction inconnue `{function_name}`"),
                            path[0].1,
                        ));
                        return Type::Unknown("<error>".to_owned());
                    };
                    self.symbols.push(SymbolInfo {
                        id: callee.id,
                        name: function_name.clone(),
                        kind: SymbolKind::Function,
                        span: path[0].1,
                        definition_span: callee.span,
                        type_name: Some(format_signature(&callee)),
                    });
                    if args.len() != callee.params.len() {
                        self.diagnostics.push(Diagnostic::error(
                            "RBN3004",
                            format!(
                                "`{function_name}` attend {} argument(s), reçu {}",
                                callee.params.len(),
                                args.len()
                            ),
                            expression.span,
                        ));
                    }
                    for (argument, (_, expected)) in args.iter().zip(&callee.params) {
                        let actual = self.infer_expression(argument, environment, effects);
                        if &actual != expected {
                            self.diagnostics.push(Diagnostic::error(
                                "RBN3001",
                                format!("`{function_name}` attend `{expected}`, reçu `{actual}`"),
                                argument.span,
                            ));
                        }
                    }
                    effects.extend(callee.declared_effects.iter().cloned());
                    callee.return_type
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN2004",
                        format!(
                            "appel inconnu `{}`",
                            path.iter()
                                .map(|(part, _)| part.as_str())
                                .collect::<Vec<_>>()
                                .join(".")
                        ),
                        expression.span,
                    ));
                    Type::Unknown("<error>".to_owned())
                }
            }
        }
    }
}

fn format_signature(function: &FunctionSummary) -> String {
    let params = function
        .params
        .iter()
        .map(|(name, type_name)| format!("{name}: {type_name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let effects = if function.declared_effects.is_empty() {
        String::new()
    } else {
        format!(
            " ! {{ {} }}",
            function
                .declared_effects
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "fn {}({params}) -> {}{effects}",
        function.name, function.return_type
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreInstruction {
    ConsoleWrite(String),
    RustGraphemeCount(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreProgram {
    pub instructions: Vec<CoreInstruction>,
}

/// Lowers the selected synchronous root to the explicit bootstrap Core.
///
/// # Errors
///
/// Returns a Robine diagnostic when analysis failed, the root is absent or a
/// construct is outside the currently published bootstrap subset.
pub fn lower_entry(analysis: &Analysis, entry: &str) -> Result<CoreProgram, Diagnostic> {
    if !analysis.is_valid() {
        return Err(Diagnostic::error(
            "RBN6000",
            "le programme doit être valide avant l’abaissement",
            Span::default(),
        ));
    }
    let Some(program) = analysis.program.as_ref() else {
        return Err(Diagnostic::error(
            "RBN6000",
            "l’analyse typée ne contient aucun programme",
            Span::default(),
        ));
    };
    let function_name = entry.rsplit('.').next().unwrap_or(entry);
    let Some(function) = program
        .functions
        .iter()
        .find(|function| function.name == function_name)
    else {
        return Err(Diagnostic::error(
            "RBN5001",
            format!("racine `{entry}` introuvable"),
            program.module_span,
        ));
    };
    let mut instructions = Vec::new();
    for statement in &function.body {
        let expression = match &statement.kind {
            StmtKind::Expr { value, .. } | StmtKind::Let { value, .. } => value,
        };
        if let ExprKind::Call { path, args } = &expression.kind
            && path.len() == 2
            && path[1].0 == "write_line"
            && let [
                Expr {
                    kind: ExprKind::Text(value),
                    ..
                },
            ] = args.as_slice()
        {
            instructions.push(CoreInstruction::ConsoleWrite(value.clone()));
        } else if let ExprKind::Call { path, args } = &expression.kind
            && path.len() == 2
            && path[0].0 == "rust"
            && path[1].0 == "grapheme_count"
            && let [
                Expr {
                    kind: ExprKind::Text(value),
                    ..
                },
            ] = args.as_slice()
        {
            instructions.push(CoreInstruction::RustGraphemeCount(value.clone()));
        } else {
            return Err(Diagnostic::error(
                "RBN6002",
                "expression non prise en charge par le backend bootstrap",
                expression.span,
            ));
        }
    }
    Ok(CoreProgram { instructions })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = r#"module hello

fn main(console: Console) -> Unit ! { Console.Write } {
    console.write_line("Hello from Robine!")
}
"#;

    #[test]
    fn types_hello() {
        let analysis = analyze(HELLO);
        assert_eq!(analysis.phase, Phase::Typed);
        assert_eq!(analysis.diagnostics, Vec::new());
        assert_eq!(
            analysis.functions[0].required_effects,
            BTreeSet::from(["Console.Write".to_owned()])
        );
    }

    #[test]
    fn reports_missing_effect() {
        let source = HELLO.replace(" ! { Console.Write }", "");
        let analysis = analyze(&source);
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN4001")
        );
    }

    #[test]
    fn reports_argument_type() {
        let source = HELLO.replace("\"Hello from Robine!\"", "42");
        let analysis = analyze(&source);
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN3001")
        );
    }

    #[test]
    fn lowers_console_write() {
        let analysis = analyze(HELLO);
        let core = lower_entry(&analysis, "hello.main").expect("hello lowers");
        assert_eq!(
            core.instructions,
            vec![CoreInstruction::ConsoleWrite(
                "Hello from Robine!".to_owned()
            )]
        );
    }
}
