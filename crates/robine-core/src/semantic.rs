use crate::{BinaryOp, Diagnostic, Expr, ExprKind, Function, Program, Span, StableId, StmtKind};
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
            ExprKind::If {
                condition,
                consequence,
                alternative,
            } => {
                let condition_type = self.infer_expression(condition, environment, effects);
                if condition_type != Type::Bool {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN3005",
                        format!("la condition de `if` attend `Bool`, reçu `{condition_type}`"),
                        condition.span,
                    ));
                }
                let consequence_type = self.infer_expression(consequence, environment, effects);
                let alternative_type = self.infer_expression(alternative, environment, effects);
                if consequence_type == alternative_type {
                    consequence_type
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN3006",
                        format!(
                            "les branches de `if` retournent `{consequence_type}` et `{alternative_type}`"
                        ),
                        expression.span,
                    ));
                    Type::Unknown("<error>".to_owned())
                }
            }
            ExprKind::Binary {
                operator,
                left,
                right,
            } => {
                let left_type = self.infer_expression(left, environment, effects);
                let right_type = self.infer_expression(right, environment, effects);
                if left_type != Type::Int {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN3007",
                        format!(
                            "l’opérateur `{}` attend `Int` à gauche, reçu `{left_type}`",
                            operator.symbol()
                        ),
                        left.span,
                    ));
                }
                if right_type != Type::Int {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN3007",
                        format!(
                            "l’opérateur `{}` attend `Int` à droite, reçu `{right_type}`",
                            operator.symbol()
                        ),
                        right.span,
                    ));
                }
                match operator {
                    BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => Type::Int,
                    BinaryOp::Equal | BinaryOp::LessThan | BinaryOp::LessThanOrEqual => Type::Bool,
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
pub struct CoreExpr {
    pub type_name: Type,
    pub kind: CoreExprKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreExprKind {
    Unit,
    Bool(bool),
    Int(i64),
    Text(String),
    Local(String),
    If {
        condition: Box<CoreExpr>,
        consequence: Box<CoreExpr>,
        alternative: Box<CoreExpr>,
    },
    Binary {
        operator: CoreBinaryOp,
        left: Box<CoreExpr>,
        right: Box<CoreExpr>,
    },
    Call {
        function: String,
        args: Vec<CoreExpr>,
    },
    ConsoleWrite {
        receiver: Box<CoreExpr>,
        text: Box<CoreExpr>,
    },
    RustGraphemeCount {
        text: Box<CoreExpr>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreBinaryOp {
    Add,
    Subtract,
    Multiply,
    Equal,
    LessThan,
    LessThanOrEqual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreStatement {
    Let { name: String, value: CoreExpr },
    Expr { value: CoreExpr, terminated: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreFunction {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<CoreStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreProgram {
    pub entry: String,
    pub functions: Vec<CoreFunction>,
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
    let entry_name = entry.rsplit('.').next().unwrap_or(entry);
    let Some(_) = program
        .functions
        .iter()
        .find(|function| function.name == entry_name)
    else {
        return Err(Diagnostic::error(
            "RBN5001",
            format!("racine `{entry}` introuvable"),
            program.module_span,
        ));
    };

    let signatures = analysis
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect::<BTreeMap<_, _>>();
    let mut functions = Vec::with_capacity(program.functions.len());
    for function in &program.functions {
        let Some(signature) = signatures.get(function.name.as_str()) else {
            return Err(Diagnostic::error(
                "RBN6001",
                format!("signature typée absente pour `{}`", function.name),
                function.name_span,
            ));
        };
        functions.push(lower_function(function, signature, &signatures)?);
    }
    Ok(CoreProgram {
        entry: entry_name.to_owned(),
        functions,
    })
}

fn lower_function(
    function: &Function,
    signature: &FunctionSummary,
    signatures: &BTreeMap<&str, &FunctionSummary>,
) -> Result<CoreFunction, Diagnostic> {
    let mut environment = signature.params.iter().cloned().collect::<BTreeMap<_, _>>();
    let mut body = Vec::with_capacity(function.body.len());
    for statement in &function.body {
        match &statement.kind {
            StmtKind::Let { name, value, .. } => {
                let value = lower_expression(value, &environment, signatures)?;
                environment.insert(name.clone(), value.type_name.clone());
                body.push(CoreStatement::Let {
                    name: name.clone(),
                    value,
                });
            }
            StmtKind::Expr { value, terminated } => {
                body.push(CoreStatement::Expr {
                    value: lower_expression(value, &environment, signatures)?,
                    terminated: *terminated,
                });
            }
        }
    }
    Ok(CoreFunction {
        name: function.name.clone(),
        params: signature.params.clone(),
        return_type: signature.return_type.clone(),
        body,
    })
}

fn lower_expression(
    expression: &Expr,
    environment: &BTreeMap<String, Type>,
    signatures: &BTreeMap<&str, &FunctionSummary>,
) -> Result<CoreExpr, Diagnostic> {
    let (type_name, kind) = match &expression.kind {
        ExprKind::Text(value) => (Type::Text, CoreExprKind::Text(value.clone())),
        ExprKind::Int(value) => (Type::Int, CoreExprKind::Int(*value)),
        ExprKind::Bool(value) => (Type::Bool, CoreExprKind::Bool(*value)),
        ExprKind::Var(name) => {
            let Some(type_name) = environment.get(name) else {
                return unsupported_core(expression, format!("binding `{name}` non résolu"));
            };
            (type_name.clone(), CoreExprKind::Local(name.clone()))
        }
        ExprKind::If {
            condition,
            consequence,
            alternative,
        } => {
            let condition = lower_expression(condition, environment, signatures)?;
            let consequence = lower_expression(consequence, environment, signatures)?;
            let alternative = lower_expression(alternative, environment, signatures)?;
            (
                consequence.type_name.clone(),
                CoreExprKind::If {
                    condition: Box::new(condition),
                    consequence: Box::new(consequence),
                    alternative: Box::new(alternative),
                },
            )
        }
        ExprKind::Binary {
            operator,
            left,
            right,
        } => {
            let left = lower_expression(left, environment, signatures)?;
            let right = lower_expression(right, environment, signatures)?;
            let (operator, result_type) = match operator {
                BinaryOp::Add => (CoreBinaryOp::Add, Type::Int),
                BinaryOp::Subtract => (CoreBinaryOp::Subtract, Type::Int),
                BinaryOp::Multiply => (CoreBinaryOp::Multiply, Type::Int),
                BinaryOp::Equal => (CoreBinaryOp::Equal, Type::Bool),
                BinaryOp::LessThan => (CoreBinaryOp::LessThan, Type::Bool),
                BinaryOp::LessThanOrEqual => (CoreBinaryOp::LessThanOrEqual, Type::Bool),
            };
            (
                result_type,
                CoreExprKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            )
        }
        ExprKind::Call { path, args } => {
            return lower_call_expression(expression, path, args, environment, signatures);
        }
    };
    Ok(CoreExpr { type_name, kind })
}

fn lower_call_expression(
    expression: &Expr,
    path: &[(String, Span)],
    args: &[Expr],
    environment: &BTreeMap<String, Type>,
    signatures: &BTreeMap<&str, &FunctionSummary>,
) -> Result<CoreExpr, Diagnostic> {
    if path.len() == 2
        && path[1].0 == "write_line"
        && args.len() == 1
        && environment.contains_key(&path[0].0)
    {
        let receiver_name = &path[0].0;
        let receiver_type = environment
            .get(receiver_name)
            .expect("receiver presence was checked");
        let receiver = CoreExpr {
            type_name: receiver_type.clone(),
            kind: CoreExprKind::Local(receiver_name.clone()),
        };
        let text = lower_expression(&args[0], environment, signatures)?;
        return Ok(CoreExpr {
            type_name: Type::Unit,
            kind: CoreExprKind::ConsoleWrite {
                receiver: Box::new(receiver),
                text: Box::new(text),
            },
        });
    }
    if path.len() == 2 && path[0].0 == "rust" && path[1].0 == "grapheme_count" && args.len() == 1 {
        let text = lower_expression(&args[0], environment, signatures)?;
        return Ok(CoreExpr {
            type_name: Type::Int,
            kind: CoreExprKind::RustGraphemeCount {
                text: Box::new(text),
            },
        });
    }
    if let [(function_name, _)] = path {
        let Some(signature) = signatures.get(function_name.as_str()) else {
            return unsupported_core(
                expression,
                format!("fonction `{function_name}` non résolue"),
            );
        };
        let args = args
            .iter()
            .map(|argument| lower_expression(argument, environment, signatures))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(CoreExpr {
            type_name: signature.return_type.clone(),
            kind: CoreExprKind::Call {
                function: function_name.clone(),
                args,
            },
        });
    }
    unsupported_core(
        expression,
        "expression non prise en charge par le Core bootstrap",
    )
}

fn unsupported_core<T>(expression: &Expr, message: impl Into<String>) -> Result<T, Diagnostic> {
    Err(Diagnostic::error("RBN6002", message, expression.span))
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
        assert_eq!(core.entry, "main");
        let main = core
            .functions
            .iter()
            .find(|function| function.name == "main")
            .expect("main Core function");
        let [CoreStatement::Expr { value, .. }] = main.body.as_slice() else {
            panic!("main should contain one Core expression");
        };
        let CoreExprKind::ConsoleWrite { text, .. } = &value.kind else {
            panic!("main should lower Console.write_line");
        };
        assert_eq!(
            text.kind,
            CoreExprKind::Text("Hello from Robine!".to_owned())
        );
    }

    #[test]
    fn types_and_lowers_if_and_user_calls() {
        let source = r#"module choice
fn choose(flag: Bool, yes: Text, no: Text) -> Text {
    if flag { yes } else { no }
}
fn main(console: Console) -> Unit ! { Console.Write } {
    console.write_line(choose(true, "yes", "no"))
}
"#;
        let analysis = analyze(source);
        assert_eq!(analysis.diagnostics, Vec::new());
        let core = lower_entry(&analysis, "choice.main").expect("choice lowers");
        let choose = core
            .functions
            .iter()
            .find(|function| function.name == "choose")
            .expect("choose Core function");
        let [CoreStatement::Expr { value, .. }] = choose.body.as_slice() else {
            panic!("choose should contain one Core expression");
        };
        assert!(matches!(value.kind, CoreExprKind::If { .. }));
    }

    #[test]
    fn reports_invalid_if_condition_and_branch_types() {
        let source = r#"module invalid
fn bad() -> Text {
    if 42 { "text" } else { false }
}
"#;
        let analysis = analyze(source);
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN3005")
        );
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN3006")
        );
    }

    #[test]
    fn reports_non_integer_arithmetic() {
        let source = r"module invalid
fn bad() -> Int {
    true + 1
}
";
        let analysis = analyze(source);
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN3007")
        );
    }
}
