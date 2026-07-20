use crate::{Analysis, Diagnostic, Span, Type, analyze};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    pub target: Targets,
    #[serde(default)]
    pub foreign: BTreeMap<String, ForeignFunction>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    #[serde(default = "default_syntax_profile")]
    pub syntax_profile: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Targets {
    pub app: AppTarget,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppTarget {
    #[serde(default = "default_app_profile")]
    pub profile: String,
    #[serde(default = "default_source")]
    pub source: String,
    pub entry: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ForeignFunction {
    pub library: String,
    pub symbol: String,
    pub abi: String,
    pub parameters: Vec<String>,
    pub result: String,
    pub panic: String,
    #[serde(default)]
    pub effects: Vec<String>,
}

fn default_source() -> String {
    "src/main.ro".to_owned()
}

fn default_app_profile() -> String {
    "app.sync-v0".to_owned()
}

fn default_domain() -> String {
    "normal".to_owned()
}

fn default_syntax_profile() -> String {
    crate::SYNTAX_PROFILE.to_owned()
}

#[must_use]
pub fn is_source_path(path: &Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str) == Some(crate::SOURCE_EXTENSION)
}

#[derive(Clone, Debug)]
pub struct LoadedProject {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub source_path: PathBuf,
    pub manifest: Manifest,
    pub source: String,
}

impl LoadedProject {
    /// Loads `robine.toml` and the source selected by its application target.
    ///
    /// # Errors
    ///
    /// Returns a contextual message when the path, manifest, TOML payload or
    /// selected source cannot be read.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let root = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .ok_or_else(|| "chemin de projet sans parent".to_owned())?
                .to_path_buf()
        };
        let manifest_path = root.join("robine.toml");
        let manifest_source = fs::read_to_string(&manifest_path).map_err(|error| {
            format!("lecture de {} impossible: {error}", manifest_path.display())
        })?;
        let manifest: Manifest = toml::from_str(&manifest_source)
            .map_err(|error| format!("manifeste {} invalide: {error}", manifest_path.display()))?;
        let source_path = root.join(&manifest.target.app.source);
        let source = fs::read_to_string(&source_path)
            .map_err(|error| format!("lecture de {} impossible: {error}", source_path.display()))?;
        Ok(Self {
            root,
            manifest_path,
            source_path,
            manifest,
            source,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ProjectAnalysis {
    pub analysis: Analysis,
    pub diagnostics: Vec<Diagnostic>,
}

impl ProjectAnalysis {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.analysis.is_valid() && self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn all_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.analysis.diagnostics.clone();
        diagnostics.extend(self.diagnostics.clone());
        diagnostics
    }
}

#[must_use]
pub fn analyze_project(project: &LoadedProject) -> ProjectAnalysis {
    let analysis = analyze(&project.source);
    let mut diagnostics = validate_manifest(&project.manifest);
    diagnostics.extend(validate_foreign_calls(&analysis, &project.manifest));
    diagnostics.extend(validate_application_entry(
        &analysis,
        &project.manifest.target.app,
    ));

    ProjectAnalysis {
        analysis,
        diagnostics,
    }
}

fn validate_manifest(manifest: &Manifest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if semver::Version::parse(&manifest.package.version).is_err() {
        diagnostics.push(Diagnostic::error(
            "RBN5006",
            format!(
                "version de package `{}` invalide; SemVer attendu",
                manifest.package.version
            ),
            Span::default(),
        ));
    }

    if manifest.package.syntax_profile != crate::SYNTAX_PROFILE {
        diagnostics.push(Diagnostic::error(
            "RBN5000",
            format!(
                "profil syntaxique `{}` non pris en charge; attendu `{}`",
                manifest.package.syntax_profile,
                crate::SYNTAX_PROFILE
            ),
            Span::default(),
        ));
    }

    if !is_source_path(Path::new(&manifest.target.app.source)) {
        diagnostics.push(Diagnostic::error(
            "RBN5008",
            format!(
                "extension source invalide pour `{}`; attendu `.{}`",
                manifest.target.app.source,
                crate::SOURCE_EXTENSION
            ),
            Span::default(),
        ));
    }

    if manifest.target.app.domain != "normal" {
        diagnostics.push(Diagnostic::error(
            "RBN5005",
            format!(
                "le bootstrap prend uniquement en charge le domaine `normal`, reçu `{}`",
                manifest.target.app.domain
            ),
            Span::default(),
        ));
    }

    if manifest.target.app.profile != "app.sync-v0" {
        diagnostics.push(Diagnostic::error(
            "RBN5007",
            format!(
                "profil d’application `{}` non pris en charge; attendu `app.sync-v0`",
                manifest.target.app.profile
            ),
            Span::default(),
        ));
    }

    diagnostics
}

fn validate_foreign_calls(analysis: &Analysis, manifest: &Manifest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for foreign_call in &analysis.foreign_calls {
        match manifest.foreign.get(foreign_call) {
            None => diagnostics.push(Diagnostic::error(
                "RBN4201",
                format!("appel étranger `{foreign_call}` absent du manifeste"),
                Span::default(),
            )),
            Some(declaration)
                if declaration.library != "robine-rust-bridge-demo"
                    || declaration.abi != "C"
                    || declaration.symbol != "robine_demo_grapheme_count"
                    || declaration.parameters != ["Text.borrowed"]
                    || declaration.result != "Int"
                    || declaration.panic != "sentinel"
                    || !declaration.effects.is_empty() =>
            {
                diagnostics.push(Diagnostic::error(
                    "RBN4202",
                    format!("contrat ABI incomplet ou incompatible pour `{foreign_call}`"),
                    Span::default(),
                ));
            }
            Some(_) => {}
        }
    }

    diagnostics
}

fn validate_application_entry(analysis: &Analysis, target: &AppTarget) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(program) = &analysis.program {
        let entry_parts = target.entry.rsplit_once('.');
        let function = entry_parts.and_then(|(module, function_name)| {
            if module == program.module {
                analysis
                    .functions
                    .iter()
                    .find(|function| function.name == function_name)
            } else {
                None
            }
        });
        if let Some(function) = function {
            let signature_is_valid = matches!(function.params.as_slice(), [(_, Type::Console)])
                && function.return_type == Type::Unit;
            if !signature_is_valid {
                diagnostics.push(Diagnostic::error(
                    "RBN5002",
                    "la racine `app.sync-v0` doit recevoir une capacité `Console` et retourner `Unit`",
                    function.span,
                ));
            }
            if function.required_effects.contains("Console.Write")
                && !target
                    .capabilities
                    .iter()
                    .any(|capability| capability == "console.write")
            {
                diagnostics.push(Diagnostic::error(
                    "RBN4101",
                    "l’effet `Console.Write` exige la capacité manifeste `console.write`",
                    function.span,
                ));
            }
        } else {
            diagnostics.push(Diagnostic::error(
                "RBN5001",
                format!("racine `{}` introuvable", target.entry),
                program.module_span,
            ));
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(capabilities: Vec<String>) -> LoadedProject {
        LoadedProject {
            root: PathBuf::new(),
            manifest_path: PathBuf::from("robine.toml"),
            source_path: PathBuf::from("src/main.ro"),
            manifest: Manifest {
                package: Package {
                    name: "hello".to_owned(),
                    version: "0.1.0".to_owned(),
                    syntax_profile: crate::SYNTAX_PROFILE.to_owned(),
                },
                target: Targets {
                    app: AppTarget {
                        profile: "app.sync-v0".to_owned(),
                        source: "src/main.ro".to_owned(),
                        entry: "hello.main".to_owned(),
                        domain: "normal".to_owned(),
                        capabilities,
                    },
                },
                foreign: BTreeMap::new(),
            },
            source: r#"module hello
fn main(console: Console) -> Unit ! { Console.Write } {
    console.write_line("Hello")
}
"#
            .to_owned(),
        }
    }

    #[test]
    fn accepts_explicit_console_capability() {
        assert!(analyze_project(&project(vec!["console.write".to_owned()])).is_valid());
    }

    #[test]
    fn rejects_missing_console_capability() {
        let result = analyze_project(&project(Vec::new()));
        assert!(result.diagnostics.iter().any(|item| item.code == "RBN4101"));
    }

    #[test]
    fn entry_capability_parameter_name_is_not_semantic() {
        let mut renamed = project(vec!["console.write".to_owned()]);
        renamed.source = renamed.source.replace("console", "terminal");
        assert!(analyze_project(&renamed).is_valid());
    }

    #[test]
    fn manifest_selects_the_root_function_by_identity() {
        let mut renamed = project(vec!["console.write".to_owned()]);
        renamed.source = renamed.source.replace("main", "start");
        renamed.manifest.target.app.entry = "hello.start".to_owned();
        assert!(analyze_project(&renamed).is_valid());
    }

    #[test]
    fn rejects_invalid_package_version_and_application_profile() {
        let mut invalid = project(vec!["console.write".to_owned()]);
        invalid.manifest.package.version = "tomorrow".to_owned();
        invalid.manifest.target.app.profile = "app.future".to_owned();
        let diagnostics = analyze_project(&invalid).diagnostics;
        assert!(diagnostics.iter().any(|item| item.code == "RBN5006"));
        assert!(diagnostics.iter().any(|item| item.code == "RBN5007"));
    }

    #[test]
    fn rejects_legacy_source_extension() {
        let mut legacy = project(vec!["console.write".to_owned()]);
        legacy.manifest.target.app.source = "src/main.robine".to_owned();
        let diagnostics = analyze_project(&legacy).diagnostics;
        assert!(diagnostics.iter().any(|item| item.code == "RBN5008"));
    }

    #[test]
    fn manifest_defaults_to_short_source_extension() {
        let manifest: Manifest = toml::from_str(
            r#"
[package]
name = "defaults"
version = "0.1.0"

[target.app]
entry = "defaults.main"
"#,
        )
        .expect("minimal manifest");
        assert_eq!(manifest.target.app.source, "src/main.ro");
    }

    #[test]
    fn foreign_call_requires_an_exact_manifest_contract() {
        let mut foreign = project(vec!["console.write".to_owned()]);
        foreign.source = foreign.source.replace(
            "console.write_line",
            "rust.grapheme_count(\"Robine\");\n    console.write_line",
        );
        let missing = analyze_project(&foreign);
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|item| item.code == "RBN4201")
        );

        foreign.manifest.foreign.insert(
            "rust.grapheme_count".to_owned(),
            ForeignFunction {
                library: "robine-rust-bridge-demo".to_owned(),
                symbol: "robine_demo_grapheme_count".to_owned(),
                abi: "C".to_owned(),
                parameters: vec!["Text.borrowed".to_owned()],
                result: "Int".to_owned(),
                panic: "sentinel".to_owned(),
                effects: Vec::new(),
            },
        );
        assert!(analyze_project(&foreign).is_valid());
    }
}
