use crate::{Expr, ExprKind, Program, StmtKind};
use std::fmt::Write as _;

#[must_use]
pub fn format_program(program: &Program) -> String {
    let mut output = String::new();
    writeln!(output, "module {}", program.module).expect("writing to a string cannot fail");
    for function in &program.functions {
        output.push('\n');
        write!(output, "fn {}(", function.name).expect("writing to a string cannot fail");
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            write!(output, "{}: {}", parameter.name, parameter.type_name)
                .expect("writing to a string cannot fail");
        }
        write!(output, ") -> {}", function.return_type).expect("writing to a string cannot fail");
        if !function.effects.is_empty() {
            output.push_str(" ! { ");
            for (index, (effect, _)) in function.effects.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                output.push_str(effect);
            }
            output.push_str(" }");
        }
        output.push_str(" {\n");
        for statement in &function.body {
            output.push_str("    ");
            match &statement.kind {
                StmtKind::Let {
                    name,
                    annotation,
                    value,
                    ..
                } => {
                    write!(output, "let {name}").expect("writing to a string cannot fail");
                    if let Some((type_name, _)) = annotation {
                        write!(output, ": {type_name}").expect("writing to a string cannot fail");
                    }
                    output.push_str(" = ");
                    format_expr(value, &mut output);
                    output.push(';');
                }
                StmtKind::Expr { value, terminated } => {
                    format_expr(value, &mut output);
                    if *terminated {
                        output.push(';');
                    }
                }
            }
            output.push('\n');
        }
        output.push_str("}\n");
    }
    output
}

/// Formats source when doing so cannot discard comments.
///
/// The bootstrap formatter deliberately leaves commented source unchanged
/// until comment attachment is represented in the structural tree.
#[must_use]
pub fn format_source(source: &str, program: &Program) -> String {
    if program.has_comments {
        source.to_owned()
    } else {
        format_program(program)
    }
}

fn format_expr(expression: &Expr, output: &mut String) {
    match &expression.kind {
        ExprKind::Text(value) => {
            output.push('"');
            for ch in value.chars() {
                match ch {
                    '\n' => output.push_str("\\n"),
                    '\r' => output.push_str("\\r"),
                    '\t' => output.push_str("\\t"),
                    '"' => output.push_str("\\\""),
                    '\\' => output.push_str("\\\\"),
                    _ => output.push(ch),
                }
            }
            output.push('"');
        }
        ExprKind::Int(value) => write!(output, "{value}").expect("writing cannot fail"),
        ExprKind::Bool(value) => write!(output, "{value}").expect("writing cannot fail"),
        ExprKind::Var(name) => output.push_str(name),
        ExprKind::Call { path, args } => {
            for (index, (part, _)) in path.iter().enumerate() {
                if index != 0 {
                    output.push('.');
                }
                output.push_str(part);
            }
            output.push('(');
            for (index, argument) in args.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                format_expr(argument, output);
            }
            output.push(')');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn formatting_is_idempotent() {
        let source = "module hello fn main(console:Console)->Unit!{Console.Write}{console.write_line(\"hi\")}";
        let once = format_program(&parse(source).expect("source should parse"));
        let twice = format_program(&parse(&once).expect("formatted source should parse"));
        assert_eq!(once, twice);
    }

    #[test]
    fn bootstrap_formatter_never_discards_comments() {
        let source = "module hello\n// keep me\nfn main() -> Unit {}\n";
        let program = parse(source).expect("source should parse");
        assert!(program.has_comments);
        assert_eq!(format_source(source, &program), source);
    }
}
