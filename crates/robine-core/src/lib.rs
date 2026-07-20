mod diagnostic;
mod engine;
mod formatter;
mod project;
mod semantic;
mod syntax;

pub use diagnostic::{Diagnostic, DiagnosticSeverity, Span};
pub use engine::{DocumentSnapshot, Engine, InvalidationReport};
pub use formatter::{format_program, format_source};
pub use project::{
    AppTarget, ForeignFunction, LoadedProject, Manifest, Package, ProjectAnalysis,
    ProjectDiagnostic, ProjectModule, SourceFile, Targets, analyze_project, is_source_path,
};
pub use semantic::{
    Analysis, CoreBinaryOp, CoreExpr, CoreExprKind, CoreFunction, CoreProgram, CoreStatement,
    FunctionSummary, Phase, SymbolInfo, SymbolKind, Type, analyze, analyze_modules, lower_entry,
    lower_modules,
};
pub use syntax::{
    BinaryOp, Expr, ExprKind, Function, Import, Param, Program, StableId, Stmt, StmtKind, parse,
};

pub const SYNTAX_PROFILE: &str = "prototype-conventional-0";
pub const SOURCE_EXTENSION: &str = "ro";
