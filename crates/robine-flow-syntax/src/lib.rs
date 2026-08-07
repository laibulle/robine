//! Lexer, parseur et rendu déterministe du texte Robine Flow.

use robine_flow_ast::{AstError, FlowAst, Form};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

pub fn parse(source: &str) -> Result<FlowAst, Vec<Diagnostic>> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens: &tokens,
        offset: 0,
    };
    let root = parser.form()?;
    if let Some(token) = parser.tokens.get(parser.offset) {
        return Err(vec![Diagnostic {
            code: "flow.trailing_form",
            message: "a Flow document contains exactly one root form".into(),
            span: token.span,
        }]);
    }
    FlowAst::new(root.value).map_err(|error| {
        vec![Diagnostic {
            code: "flow.root",
            message: error.to_string(),
            span: root.span,
        }]
    })
}

pub fn format(ast: &FlowAst) -> Result<String, AstError> {
    ast.validate()?;
    let mut output = String::new();
    format_form(&ast.root, 0, &mut output);
    output.push('\n');
    Ok(output)
}

#[derive(Clone, Debug)]
struct SpannedForm {
    value: Form,
    span: Span,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

#[derive(Clone, Debug)]
enum TokenKind {
    Open(Delimiter),
    Close(Delimiter),
    String(String),
    Atom(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delimiter {
    Parenthesis,
    Bracket,
}

fn lex(source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character.is_whitespace() {
            continue;
        }
        if character == ';' {
            for (_, character) in chars.by_ref() {
                if character == '\n' {
                    break;
                }
            }
            continue;
        }
        match character {
            '(' => tokens.push(Token {
                kind: TokenKind::Open(Delimiter::Parenthesis),
                span: Span {
                    start,
                    end: start + 1,
                },
            }),
            ')' => tokens.push(Token {
                kind: TokenKind::Close(Delimiter::Parenthesis),
                span: Span {
                    start,
                    end: start + 1,
                },
            }),
            '[' => tokens.push(Token {
                kind: TokenKind::Open(Delimiter::Bracket),
                span: Span {
                    start,
                    end: start + 1,
                },
            }),
            ']' => tokens.push(Token {
                kind: TokenKind::Close(Delimiter::Bracket),
                span: Span {
                    start,
                    end: start + 1,
                },
            }),
            '"' => tokens.push(Token {
                kind: TokenKind::String(read_string(source, start, &mut chars)?),
                span: Span {
                    start,
                    end: chars
                        .peek()
                        .map(|(index, _)| *index)
                        .unwrap_or(source.len()),
                },
            }),
            _ => {
                let mut atom = String::from(character);
                let mut end = start + character.len_utf8();
                while let Some((index, next)) = chars.peek().copied() {
                    if next.is_whitespace() || matches!(next, '(' | ')' | '[' | ']' | ';') {
                        break;
                    }
                    chars.next();
                    atom.push(next);
                    end = index + next.len_utf8();
                }
                tokens.push(Token {
                    kind: TokenKind::Atom(atom),
                    span: Span { start, end },
                });
            }
        }
    }
    Ok(tokens)
}

fn read_string(
    source: &str,
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Result<String, Vec<Diagnostic>> {
    let mut value = String::new();
    while let Some((index, character)) = chars.next() {
        match character {
            '"' => return Ok(value),
            '\\' => {
                let Some((escape_index, escape)) = chars.next() else {
                    return Err(vec![syntax_error(
                        "flow.unterminated_string",
                        "unterminated string escape",
                        Span {
                            start,
                            end: source.len(),
                        },
                    )]);
                };
                match escape {
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    '/' => value.push('/'),
                    'b' => value.push('\u{0008}'),
                    'f' => value.push('\u{000c}'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'u' => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            let Some((_, digit)) = chars.next() else {
                                return Err(vec![syntax_error(
                                    "flow.invalid_escape",
                                    "incomplete unicode escape",
                                    Span {
                                        start: escape_index,
                                        end: source.len(),
                                    },
                                )]);
                            };
                            hex.push(digit);
                        }
                        let code = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| {
                                vec![syntax_error(
                                    "flow.invalid_escape",
                                    "invalid unicode escape",
                                    Span {
                                        start: escape_index,
                                        end: escape_index + 5,
                                    },
                                )]
                            })?;
                        value.push(code);
                    }
                    _ => {
                        return Err(vec![syntax_error(
                            "flow.invalid_escape",
                            "invalid string escape",
                            Span {
                                start: escape_index,
                                end: escape_index + escape.len_utf8(),
                            },
                        )]);
                    }
                }
            }
            _ => {
                value.push(character);
                let _ = index;
            }
        }
    }
    Err(vec![syntax_error(
        "flow.unterminated_string",
        "unterminated string",
        Span {
            start,
            end: source.len(),
        },
    )])
}

struct Parser<'a> {
    tokens: &'a [Token],
    offset: usize,
}

impl Parser<'_> {
    fn form(&mut self) -> Result<SpannedForm, Vec<Diagnostic>> {
        let Some(token) = self.tokens.get(self.offset) else {
            return Err(vec![syntax_error(
                "flow.unexpected_eof",
                "expected a form",
                Span { start: 0, end: 0 },
            )]);
        };
        self.offset += 1;
        match &token.kind {
            TokenKind::Close(_) => Err(vec![syntax_error(
                "flow.unexpected_close",
                "unexpected closing parenthesis",
                token.span,
            )]),
            TokenKind::String(value) => Ok(SpannedForm {
                value: Form::String(value.clone()),
                span: token.span,
            }),
            TokenKind::Atom(value) => Ok(SpannedForm {
                value: atom(value),
                span: token.span,
            }),
            TokenKind::Open(delimiter) => {
                let mut forms = Vec::new();
                loop {
                    let Some(next) = self.tokens.get(self.offset) else {
                        return Err(vec![syntax_error(
                            "flow.unclosed_list",
                            "unclosed list",
                            token.span,
                        )]);
                    };
                    if matches!(next.kind, TokenKind::Close(close) if close == *delimiter) {
                        self.offset += 1;
                        return Ok(SpannedForm {
                            value: Form::List(forms),
                            span: Span {
                                start: token.span.start,
                                end: next.span.end,
                            },
                        });
                    }
                    if matches!(next.kind, TokenKind::Close(_)) {
                        return Err(vec![syntax_error(
                            "flow.mismatched_delimiter",
                            "mismatched list delimiter",
                            next.span,
                        )]);
                    }
                    forms.push(self.form()?.value);
                }
            }
        }
    }
}

fn atom(value: &str) -> Form {
    match value {
        "true" => Form::Bool(true),
        "false" => Form::Bool(false),
        "nil" => Form::Nil,
        _ if value.starts_with(':') && value.len() > 1 => Form::Keyword(value[1..].into()),
        _ => parse_number(value).unwrap_or_else(|| Form::Symbol(value.into())),
    }
}

fn parse_number(value: &str) -> Option<Form> {
    let unit_start = value
        .char_indices()
        .find_map(|(index, character)| {
            (!character.is_ascii_digit() && character != '.' && character != '-').then_some(index)
        })
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(unit_start);
    if number.is_empty() || number == "-" || number.parse::<f64>().is_err() {
        return None;
    }
    let unit = (!unit.is_empty()).then_some(unit.to_owned());
    Some(Form::Number {
        literal: number.into(),
        unit,
    })
}

fn format_form(form: &Form, indent: usize, output: &mut String) {
    match form {
        Form::List(forms) => {
            if forms.is_empty() {
                output.push_str("()");
                return;
            }
            let inline = forms.iter().all(Form::is_atom);
            output.push('(');
            for (index, form) in forms.iter().enumerate() {
                if index > 0 {
                    if inline {
                        output.push(' ');
                    } else {
                        output.push('\n');
                        output.push_str(&" ".repeat(indent + 2));
                    }
                }
                format_form(form, if inline { indent } else { indent + 2 }, output);
            }
            output.push(')');
        }
        Form::Symbol(value) => output.push_str(value),
        Form::Keyword(value) => {
            output.push(':');
            output.push_str(value);
        }
        Form::String(value) => {
            output.push('"');
            for character in value.chars() {
                match character {
                    '"' => output.push_str("\\\""),
                    '\\' => output.push_str("\\\\"),
                    '\n' => output.push_str("\\n"),
                    '\r' => output.push_str("\\r"),
                    '\t' => output.push_str("\\t"),
                    character if character.is_control() => {
                        output.push_str(&format!("\\u{:04x}", character as u32))
                    }
                    character => output.push(character),
                }
            }
            output.push('"');
        }
        Form::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Form::Nil => output.push_str("nil"),
        Form::Number { literal, unit } => {
            output.push_str(literal);
            if let Some(unit) = unit {
                output.push_str(unit);
            }
        }
    }
}

fn syntax_error(code: &'static str, message: &str, span: Span) -> Diagnostic {
    Diagnostic {
        code,
        message: message.into(),
        span,
    }
}

#[derive(Debug, Error)]
pub enum SyntaxError {
    #[error("Flow syntax error")]
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_a_flow_deterministically() {
        let source = r#"; A cosy first habit
(flow
  (meta :name "Entrée" :mode :restart :max-runtime 10m)
  (on (state-changed (entity "ent_motion") :motion :to true))
  (when (< (state (entity "ent_lux") :illuminance) 20%))
  (do (command (entity "ent_light") :turn-on :brightness 40%)))"#;
        let ast = parse(source).unwrap();
        let formatted = format(&ast).unwrap();
        assert_eq!(parse(&formatted).unwrap(), ast);
        assert!(formatted.contains("20%"));
    }

    #[test]
    fn accepts_brackets_as_a_canonical_list_delimiter() {
        let ast =
            parse(r#"(flow (on (schedule :at "09:15" :weekdays [mon wed] :timezone "UTC")) (do))"#)
                .unwrap();
        let weekdays = ast.root.list().unwrap()[1].list().unwrap()[1]
            .list()
            .unwrap()[4]
            .list()
            .unwrap();
        assert!(
            matches!(weekdays, [Form::Symbol(mon), Form::Symbol(wed)] if mon == "mon" && wed == "wed")
        );
        assert!(format(&ast).unwrap().contains("(mon wed)"));
    }

    #[test]
    fn rejects_unclosed_lists_with_a_diagnostic() {
        let error = parse("(flow (on (event :type x))").unwrap_err();
        assert_eq!(error[0].code, "flow.unclosed_list");
    }
}
