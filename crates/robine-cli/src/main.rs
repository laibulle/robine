mod lsp;

use anyhow::{Context, Result, bail};
use robine_codegen_cranelift::{StandardConsole, run_jit};
use robine_core::{
    Diagnostic, DiagnosticSeverity, LoadedProject, Span, analyze_project, format_source,
    lower_entry, parse,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("robine: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let Some(command) = arguments.next() else {
        print_help();
        return Ok(());
    };
    match command.as_str() {
        "check" => {
            let path = arguments.next().unwrap_or_else(|| ".".to_owned());
            check(Path::new(&path))
        }
        "run" => {
            let path = arguments.next().unwrap_or_else(|| ".".to_owned());
            run_project(Path::new(&path))
        }
        "fmt" => {
            let path = arguments.next().unwrap_or_else(|| ".".to_owned());
            let check_only = arguments.any(|argument| argument == "--check");
            format(Path::new(&path), check_only)
        }
        "lsp" => lsp::serve(),
        "--version" | "-V" => {
            println!("robine {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "--help" | "-h" | "help" => {
            print_help();
            Ok(())
        }
        other => bail!("commande inconnue `{other}`; utiliser `robine --help`"),
    }
}

fn print_help() {
    println!(
        "Robine bootstrap compiler

Usage:
  robine check [PROJECT]
  robine run [PROJECT]
  robine fmt [PROJECT|SOURCE] [--check]
  robine lsp --stdio

Syntax profile: {}",
        robine_core::SYNTAX_PROFILE
    );
}

fn check(path: &Path) -> Result<()> {
    let project = LoadedProject::load(path).map_err(anyhow::Error::msg)?;
    let result = analyze_project(&project);
    let diagnostics = result.all_diagnostics();
    render_diagnostics(&project.source_path, &project.source, &diagnostics);
    if diagnostics.is_empty() {
        println!("OK — {}", project.source_path.display());
        Ok(())
    } else {
        bail!("{} diagnostic(s)", diagnostics.len())
    }
}

fn run_project(path: &Path) -> Result<()> {
    let project = LoadedProject::load(path).map_err(anyhow::Error::msg)?;
    let result = analyze_project(&project);
    let diagnostics = result.all_diagnostics();
    if !diagnostics.is_empty() {
        render_diagnostics(&project.source_path, &project.source, &diagnostics);
        bail!("le programme n’est pas exécutable");
    }
    let core = lower_entry(&result.analysis, &project.manifest.target.app.entry).map_err(
        |diagnostic| {
            render_diagnostics(
                &project.source_path,
                &project.source,
                std::slice::from_ref(&diagnostic),
            );
            anyhow::anyhow!("abaissement Core impossible")
        },
    )?;
    let status = run_jit(&core, &mut StandardConsole)?;
    if status == 0 {
        Ok(())
    } else {
        bail!("le programme s’est terminé avec le statut {status}")
    }
}

fn format(path: &Path, check_only: bool) -> Result<()> {
    let source_path = source_path(path)?;
    let source = fs::read_to_string(&source_path)
        .with_context(|| format!("lecture de {}", source_path.display()))?;
    let program = parse(&source).map_err(|diagnostics| {
        render_diagnostics(&source_path, &source, &diagnostics);
        anyhow::anyhow!("formatage refusé sur une syntaxe invalide")
    })?;
    let formatted = format_source(&source, &program);
    if check_only {
        if source == formatted {
            Ok(())
        } else {
            bail!("{} n’est pas formaté", source_path.display())
        }
    } else if source == formatted {
        Ok(())
    } else {
        fs::write(&source_path, formatted)
            .with_context(|| format!("écriture de {}", source_path.display()))
    }
}

fn source_path(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    LoadedProject::load(path)
        .map(|project| project.source_path)
        .map_err(anyhow::Error::msg)
}

fn render_diagnostics(path: &Path, source: &str, diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        let (line, column) = line_column(source, diagnostic.span.start);
        let severity = match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        };
        eprintln!(
            "{}:{}:{}: {severity}[{}]: {}",
            path.display(),
            line + 1,
            column + 1,
            diagnostic.code,
            diagnostic.message
        );
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let safe_offset = offset.min(source.len());
    let before = &source[..safe_offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = source[line_start..safe_offset].chars().count();
    (line, column)
}

#[allow(dead_code)]
fn _span_for_cli(span: Span) -> Span {
    span
}
