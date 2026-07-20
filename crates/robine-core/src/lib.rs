mod diagnostic;
mod engine;
mod formatter;
mod project;
mod semantic;
mod syntax;

pub use diagnostic::{Diagnostic, DiagnosticSeverity, Span};
pub use engine::{DocumentSnapshot, Engine};
pub use formatter::{format_program, format_source};
pub use project::{
    AppTarget, ForeignFunction, LoadedProject, Manifest, Package, ProjectAnalysis, Targets,
    analyze_project,
};
pub use semantic::{
    Analysis, CoreInstruction, CoreProgram, FunctionSummary, Phase, SymbolInfo, SymbolKind, Type,
    analyze, lower_entry,
};
pub use syntax::{Expr, ExprKind, Function, Param, Program, StableId, Stmt, StmtKind, parse};

pub const SYNTAX_PROFILE: &str = "prototype-conventional-0";
