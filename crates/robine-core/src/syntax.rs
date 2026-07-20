use crate::{Diagnostic, Span};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct StableId(pub u64);

impl StableId {
    #[must_use]
    pub fn named(namespace: &str, name: &str) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in namespace.bytes().chain([0xff]).chain(name.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(hash)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub module: String,
    pub module_span: Span,
    pub functions: Vec<Function>,
    pub span: Span,
    pub has_comments: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub id: StableId,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: String,
    pub return_type_span: Span,
    pub effects: Vec<(String, Span)>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub id: StableId,
    pub name: String,
    pub name_span: Span,
    pub type_name: String,
    pub type_span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Let {
        id: StableId,
        name: String,
        name_span: Span,
        annotation: Option<(String, Span)>,
        value: Expr,
    },
    Expr {
        value: Expr,
        terminated: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Equal,
    LessThan,
    LessThanOrEqual,
}

impl BinaryOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Equal => "==",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Text(String),
    Int(i64),
    Bool(bool),
    Var(String),
    If {
        condition: Box<Expr>,
        consequence: Box<Expr>,
        alternative: Box<Expr>,
    },
    Binary {
        operator: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        path: Vec<(String, Span)>,
        args: Vec<Expr>,
    },
}

/// Parses one module with the same Tree-sitter grammar consumed by editors.
///
/// # Errors
///
/// Returns syntax diagnostics when Tree-sitter reports an error or when a
/// grammar node cannot be converted into the typed bootstrap AST.
pub fn parse(source: &str) -> Result<Program, Vec<Diagnostic>> {
    parse_incremental(source, None).0
}

pub(crate) fn parse_incremental(
    source: &str,
    old_tree: Option<&Tree>,
) -> (Result<Program, Vec<Diagnostic>>, Option<Tree>) {
    let mut parser = Parser::new();
    if let Err(error) = parser.set_language(&tree_sitter_robine::LANGUAGE.into()) {
        return (
            Err(vec![Diagnostic::error(
                "RBN1106",
                format!("chargement de la grammaire Robine impossible: {error}"),
                Span::default(),
            )]),
            None,
        );
    }
    let Some(tree) = parser.parse(source, old_tree) else {
        return (
            Err(vec![Diagnostic::error(
                "RBN1106",
                "Tree-sitter n’a produit aucun arbre",
                Span::default(),
            )]),
            None,
        );
    };
    let root = tree.root_node();
    let mut diagnostics = Vec::new();
    collect_syntax_errors(root, source, &mut diagnostics);
    if !diagnostics.is_empty() {
        return (Err(diagnostics), Some(tree));
    }
    let result = AstBuilder::new(source).program(root);
    (result, Some(tree))
}

struct AstBuilder<'source> {
    source: &'source str,
    diagnostics: Vec<Diagnostic>,
}

impl<'source> AstBuilder<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            diagnostics: Vec::new(),
        }
    }

    fn program(mut self, root: Node<'_>) -> Result<Program, Vec<Diagnostic>> {
        let children = named_children(root);
        let Some(module_node) = children
            .iter()
            .copied()
            .find(|node| node.kind() == "module_declaration")
        else {
            return Err(vec![Diagnostic::error(
                "RBN1105",
                "déclaration `module` attendue",
                span(root),
            )]);
        };
        let module_name_node = self.required_field(module_node, "name", "nom de module attendu");
        let (module, module_span) = self.text_and_span(module_name_node);
        let functions = children
            .into_iter()
            .filter(|node| node.kind() == "function_declaration")
            .map(|node| self.function(node, &module))
            .collect();
        let program = Program {
            module,
            module_span,
            functions,
            span: span(root),
            has_comments: contains_kind(root, "comment"),
        };
        if self.diagnostics.is_empty() {
            Ok(program)
        } else {
            Err(self.diagnostics)
        }
    }

    fn function(&mut self, node: Node<'_>, module: &str) -> Function {
        let name_node = self.required_field(node, "name", "nom de fonction attendu");
        let (name, name_span) = self.text_and_span(name_node);
        let return_node = self.required_field(node, "return_type", "type de retour attendu");
        let (return_type, return_type_span) = self.text_and_span(return_node);

        let mut params = Vec::new();
        let mut effects = Vec::new();
        for child in named_children(node) {
            match child.kind() {
                "parameter" => {
                    let name_node = self.required_field(child, "name", "nom de paramètre attendu");
                    let type_node = self.required_field(child, "type", "type de paramètre attendu");
                    let (param_name, param_span) = self.text_and_span(name_node);
                    let (type_name, type_span) = self.text_and_span(type_node);
                    let ordinal = params.len();
                    params.push(Param {
                        id: StableId::named(
                            &format!("{module}.{name}.param.{ordinal}"),
                            &param_name,
                        ),
                        name: param_name,
                        name_span: param_span,
                        type_name,
                        type_span,
                    });
                }
                "effect_row" => {
                    effects.extend(
                        named_children(child)
                            .into_iter()
                            .filter(|effect| effect.kind() == "qualified_identifier")
                            .map(|effect| self.text_and_span(effect)),
                    );
                }
                _ => {}
            }
        }

        let body_node = self.required_field(node, "body", "corps de fonction attendu");
        let body = self.block(body_node, module, &name);
        Function {
            id: StableId::named(module, &name),
            name,
            name_span,
            params,
            return_type,
            return_type_span,
            effects,
            body,
            span: span(node),
        }
    }

    fn block(&mut self, node: Node<'_>, module: &str, function: &str) -> Vec<Stmt> {
        let mut statements = Vec::new();
        let mut local_ordinal = 0_usize;
        for child in named_children(node) {
            match child.kind() {
                "let_statement" => {
                    let name_node = self.required_field(child, "name", "nom local attendu");
                    let value_node = self.required_field(child, "value", "valeur locale attendue");
                    let (name, name_span) = self.text_and_span(name_node);
                    let annotation = child
                        .child_by_field_name("type")
                        .map(|type_node| self.text_and_span(type_node));
                    let value = self.expression(value_node);
                    statements.push(Stmt {
                        kind: StmtKind::Let {
                            id: StableId::named(
                                &format!("{module}.{function}.local.{local_ordinal}"),
                                &name,
                            ),
                            name,
                            name_span,
                            annotation,
                            value,
                        },
                        span: span(child),
                    });
                    local_ordinal += 1;
                }
                "expression_statement" => {
                    let expression_node = named_children(child)
                        .into_iter()
                        .find(|candidate| candidate.kind() == "expression")
                        .unwrap_or(child);
                    statements.push(Stmt {
                        kind: StmtKind::Expr {
                            value: self.expression(expression_node),
                            terminated: true,
                        },
                        span: span(child),
                    });
                }
                "expression" => statements.push(Stmt {
                    kind: StmtKind::Expr {
                        value: self.expression(child),
                        terminated: false,
                    },
                    span: span(child),
                }),
                _ => {}
            }
        }
        statements
    }

    fn expression(&mut self, mut node: Node<'_>) -> Expr {
        while node.kind() == "expression" {
            let Some(inner) = node.named_child(0) else {
                self.diagnostics.push(Diagnostic::error(
                    "RBN1103",
                    "expression attendue",
                    span(node),
                ));
                return Expr {
                    kind: ExprKind::Var("<error>".to_owned()),
                    span: span(node),
                };
            };
            node = inner;
        }
        let node_span = span(node);
        let kind = match node.kind() {
            "string" => ExprKind::Text(decode_string(self.node_text(node))),
            "integer" => {
                if let Ok(value) = self.node_text(node).parse() {
                    ExprKind::Int(value)
                } else {
                    self.diagnostics.push(Diagnostic::error(
                        "RBN1003",
                        "entier hors de la plage `Int` du profil bootstrap",
                        node_span,
                    ));
                    ExprKind::Int(0)
                }
            }
            "boolean" => ExprKind::Bool(self.node_text(node) == "true"),
            "identifier" => ExprKind::Var(self.node_text(node).to_owned()),
            "if_expression" => self.if_expression(node),
            "binary_expression" => self.binary_expression(node),
            "parenthesized_expression" => {
                let value = self.required_field(node, "value", "expression parenthésée attendue");
                return self.expression(value);
            }
            "call_expression" => self.call_expression(node),
            other => {
                self.diagnostics.push(Diagnostic::error(
                    "RBN1103",
                    format!("expression `{other}` non prise en charge"),
                    node_span,
                ));
                ExprKind::Var("<error>".to_owned())
            }
        };
        Expr {
            kind,
            span: node_span,
        }
    }

    fn binary_expression(&mut self, node: Node<'_>) -> ExprKind {
        let left_node = self.required_field(node, "left", "opérande gauche attendu");
        let right_node = self.required_field(node, "right", "opérande droit attendu");
        let Some(operator_node) = node.child_by_field_name("operator") else {
            self.diagnostics.push(Diagnostic::error(
                "RBN1105",
                "opérateur binaire attendu",
                span(node),
            ));
            return ExprKind::Var("<error>".to_owned());
        };
        let operator = match self.node_text(operator_node) {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Subtract,
            "*" => BinaryOp::Multiply,
            "==" => BinaryOp::Equal,
            "<" => BinaryOp::LessThan,
            "<=" => BinaryOp::LessThanOrEqual,
            other => {
                self.diagnostics.push(Diagnostic::error(
                    "RBN1104",
                    format!("opérateur `{other}` non pris en charge"),
                    span(operator_node),
                ));
                BinaryOp::Add
            }
        };
        ExprKind::Binary {
            operator,
            left: Box::new(self.expression(left_node)),
            right: Box::new(self.expression(right_node)),
        }
    }

    fn if_expression(&mut self, node: Node<'_>) -> ExprKind {
        let condition_node = self.required_field(node, "condition", "condition de `if` attendue");
        let consequence_node =
            self.required_field(node, "consequence", "branche vraie de `if` attendue");
        let alternative_node =
            self.required_field(node, "alternative", "branche fausse de `if` attendue");
        ExprKind::If {
            condition: Box::new(self.expression(condition_node)),
            consequence: Box::new(self.expression_block(consequence_node)),
            alternative: Box::new(self.expression_block(alternative_node)),
        }
    }

    fn expression_block(&mut self, node: Node<'_>) -> Expr {
        let result_node = self.required_field(node, "result", "expression de branche attendue");
        self.expression(result_node)
    }

    fn call_expression(&mut self, node: Node<'_>) -> ExprKind {
        let path_node = self.required_field(node, "function", "fonction appelée attendue");
        let path = named_children(path_node)
            .into_iter()
            .filter(|part| part.kind() == "identifier")
            .map(|part| self.text_and_span(part))
            .collect();
        let args = named_children(node)
            .into_iter()
            .filter(|child| child.kind() == "expression")
            .map(|argument| self.expression(argument))
            .collect();
        ExprKind::Call { path, args }
    }

    fn required_field<'tree>(
        &mut self,
        node: Node<'tree>,
        field: &str,
        message: &str,
    ) -> Node<'tree> {
        if let Some(child) = node.child_by_field_name(field) {
            child
        } else {
            self.diagnostics
                .push(Diagnostic::error("RBN1105", message, span(node)));
            node
        }
    }

    fn text_and_span(&self, node: Node<'_>) -> (String, Span) {
        (self.node_text(node).to_owned(), span(node))
    }

    fn node_text(&self, node: Node<'_>) -> &str {
        self.source.get(node.byte_range()).unwrap_or("")
    }
}

fn collect_syntax_errors(node: Node<'_>, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        let node_span = span(node);
        let fragment = source
            .get(node.byte_range())
            .unwrap_or("")
            .trim()
            .chars()
            .take(24)
            .collect::<String>();
        let message = if node.is_missing() {
            format!("élément syntaxique `{}` manquant", node.kind())
        } else if fragment.is_empty() {
            "syntaxe incomplète".to_owned()
        } else {
            format!("syntaxe inattendue près de `{fragment}`")
        };
        diagnostics.push(Diagnostic::error("RBN1100", message, node_span));
        return;
    }
    for child in named_children(node) {
        collect_syntax_errors(child, source, diagnostics);
    }
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn contains_kind(node: Node<'_>, kind: &str) -> bool {
    node.kind() == kind
        || named_children(node)
            .into_iter()
            .any(|child| contains_kind(child, kind))
}

fn span(node: Node<'_>) -> Span {
    let range = node.byte_range();
    Span::new(range.start, range.end)
}

fn decode_string(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw);
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('"') => output.push('"'),
            Some('\\') | None => output.push('\\'),
            Some(other) => output.push(other),
        }
    }
    output
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
    fn parses_bootstrap_program() {
        let program = parse(HELLO).expect("hello should parse");
        assert_eq!(program.module, "hello");
        assert_eq!(program.functions[0].name, "main");
        assert_eq!(program.functions[0].effects[0].0, "Console.Write");
    }

    #[test]
    fn stable_function_id_ignores_source_position() {
        let first = parse(HELLO).expect("first parse");
        let second = parse(&format!("\n{HELLO}")).expect("second parse");
        assert_eq!(first.functions[0].id, second.functions[0].id);
    }

    #[test]
    fn rejects_incomplete_text() {
        let diagnostics = parse("module bad\nfn main() -> Text { \"unterminated }")
            .expect_err("source is incomplete");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RBN1100")
        );
    }

    #[test]
    fn parses_real_rust_bridge_example() {
        let source = include_str!("../../../examples/rust-bridge/src/main.ro");
        let program = parse(source).expect("Rust bridge example should parse");
        assert_eq!(program.functions[0].body.len(), 2);
    }

    #[test]
    fn parses_if_as_an_expression() {
        let source = r#"module choice
fn choose(flag: Bool) -> Text {
    if flag { "yes" } else { "no" }
}
"#;
        let program = parse(source).expect("if expression should parse");
        let StmtKind::Expr { value, .. } = &program.functions[0].body[0].kind else {
            panic!("function result should be an expression");
        };
        assert!(matches!(value.kind, ExprKind::If { .. }));
    }

    #[test]
    fn arithmetic_uses_the_profile_precedence() {
        let source = r"module math
fn answer() -> Bool {
    1 + 2 * 3 == 7
}
";
        let program = parse(source).expect("arithmetic should parse");
        let StmtKind::Expr { value, .. } = &program.functions[0].body[0].kind else {
            panic!("function result should be an expression");
        };
        let ExprKind::Binary {
            operator: BinaryOp::Equal,
            left,
            ..
        } = &value.kind
        else {
            panic!("equality should be the outer expression");
        };
        let ExprKind::Binary {
            operator: BinaryOp::Add,
            right,
            ..
        } = &left.kind
        else {
            panic!("addition should bind below equality");
        };
        assert!(matches!(
            right.kind,
            ExprKind::Binary {
                operator: BinaryOp::Multiply,
                ..
            }
        ));
    }
}
