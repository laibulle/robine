use crate::{Analysis, Diagnostic, Span, Type, analyze_modules};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    #[serde(default = "default_source_root")]
    pub source_root: String,
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

fn default_source_root() -> String {
    "src".to_owned()
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
pub struct SourceFile {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct LoadedProject {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_source: String,
    pub source_path: PathBuf,
    pub manifest: Manifest,
    pub source: String,
    pub sources: Vec<SourceFile>,
}

impl LoadedProject {
    /// Loads `robine.toml` and every `.ro` file below the target source root.
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
        let source_root =
            safe_project_path(&root, &manifest.target.app.source_root, "source_root")?;
        let source_path = root.join(&manifest.target.app.source);
        if !safe_relative_path(Path::new(&manifest.target.app.source)) {
            return Err(format!(
                "chemin source principal `{}` invalide",
                manifest.target.app.source
            ));
        }
        if !source_path.starts_with(&source_root) {
            return Err(format!(
                "source principal `{}` extérieur à la racine `{}`",
                manifest.target.app.source, manifest.target.app.source_root
            ));
        }
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("lecture de {} impossible: {error}", source_path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "le source principal `{}` ne peut pas être un lien symbolique",
                manifest.target.app.source
            ));
        }
        let mut sources = Vec::new();
        discover_sources(&root, &source_root, &mut sources)?;
        if sources.is_empty() {
            return Err(format!(
                "racine source `{}` sans fichier `.ro`",
                manifest.target.app.source_root
            ));
        }
        let source = sources
            .iter()
            .find(|file| file.path == source_path)
            .map(|file| file.source.clone())
            .ok_or_else(|| {
                format!(
                    "source principal `{}` absent des fichiers `.ro` découverts",
                    manifest.target.app.source
                )
            })?;
        Ok(Self {
            root,
            manifest_path,
            manifest_source,
            source_path,
            manifest,
            source,
            sources,
        })
    }

    #[must_use]
    pub fn source_for_path(&self, path: &Path) -> Option<&str> {
        if path == self.manifest_path {
            return Some(&self.manifest_source);
        }
        self.sources
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.source.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct ProjectModule {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub analysis: Analysis,
}

#[derive(Clone, Debug)]
pub struct ProjectDiagnostic {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

#[derive(Clone, Debug)]
pub struct ProjectAnalysis {
    pub modules: Vec<ProjectModule>,
    pub diagnostics: Vec<ProjectDiagnostic>,
}

impl ProjectAnalysis {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.modules.iter().all(|module| module.analysis.is_valid()) && self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn all_diagnostics(&self) -> Vec<ProjectDiagnostic> {
        let mut diagnostics = self
            .modules
            .iter()
            .flat_map(|module| {
                module
                    .analysis
                    .diagnostics
                    .iter()
                    .cloned()
                    .map(|diagnostic| ProjectDiagnostic {
                        path: module.path.clone(),
                        diagnostic,
                    })
            })
            .collect::<Vec<_>>();
        diagnostics.extend(self.diagnostics.clone());
        diagnostics
    }

    #[must_use]
    pub fn analyses(&self) -> Vec<Analysis> {
        self.modules
            .iter()
            .map(|module| module.analysis.clone())
            .collect()
    }
}

#[must_use]
pub fn analyze_project(project: &LoadedProject) -> ProjectAnalysis {
    let sources = project
        .sources
        .iter()
        .map(|file| file.source.clone())
        .collect::<Vec<_>>();
    let analyses = analyze_modules(&sources);
    let modules = project
        .sources
        .iter()
        .zip(analyses)
        .map(|(file, analysis)| ProjectModule {
            path: file.path.clone(),
            relative_path: file.relative_path.clone(),
            analysis,
        })
        .collect::<Vec<_>>();
    let mut diagnostics = validate_manifest(&project.manifest)
        .into_iter()
        .map(|diagnostic| ProjectDiagnostic {
            path: project.manifest_path.clone(),
            diagnostic,
        })
        .collect::<Vec<_>>();
    diagnostics.extend(
        validate_foreign_calls(&modules, &project.manifest)
            .into_iter()
            .map(|diagnostic| ProjectDiagnostic {
                path: project.manifest_path.clone(),
                diagnostic,
            }),
    );
    diagnostics.extend(validate_application_entry(
        &modules,
        &project.manifest.target.app,
        &project.manifest_path,
    ));

    ProjectAnalysis {
        modules,
        diagnostics,
    }
}

fn safe_project_path(root: &Path, relative: &str, field: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if !safe_relative_path(relative) {
        return Err(format!(
            "chemin `{field}` invalide: `{}`",
            relative.display()
        ));
    }
    let path = root.join(relative);
    if !path.is_dir() {
        return Err(format!("racine source `{}` absente", relative.display()));
    }
    Ok(path)
}

fn safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn discover_sources(
    root: &Path,
    directory: &Path,
    output: &mut Vec<SourceFile>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("lecture de {} impossible: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("lecture de {} impossible: {error}", directory.display()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("type de {} inaccessible: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            discover_sources(root, &entry.path(), output)?;
        } else if file_type.is_file() && is_source_path(&entry.path()) {
            let path = entry.path();
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("lecture de {} impossible: {error}", path.display()))?;
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_| format!("source {} extérieure au projet", path.display()))?
                .to_path_buf();
            output.push(SourceFile {
                path,
                relative_path,
                source,
            });
        }
    }
    output.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(())
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

fn validate_foreign_calls(modules: &[ProjectModule], manifest: &Manifest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for foreign_call in modules
        .iter()
        .flat_map(|module| &module.analysis.foreign_calls)
        .collect::<std::collections::BTreeSet<_>>()
    {
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

fn validate_application_entry(
    modules: &[ProjectModule],
    target: &AppTarget,
    manifest_path: &Path,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();

    let entry_parts = target.entry.rsplit_once('.');
    let owner = entry_parts.and_then(|(module_name, _)| {
        modules.iter().find(|module| {
            module
                .analysis
                .program
                .as_ref()
                .is_some_and(|program| program.module == module_name)
        })
    });
    if let Some(owner) = owner {
        let function = entry_parts.and_then(|(_, function_name)| {
            owner
                .analysis
                .functions
                .iter()
                .find(|function| function.name == function_name)
        });
        if let Some(function) = function {
            let signature_is_valid = matches!(function.params.as_slice(), [(_, Type::Console)])
                && function.return_type == Type::Unit;
            if !signature_is_valid {
                diagnostics.push(ProjectDiagnostic {
                    path: owner.path.clone(),
                    diagnostic: Diagnostic::error(
                        "RBN5002",
                        "la racine `app.sync-v0` doit recevoir une capacité `Console` et retourner `Unit`",
                        function.span,
                    ),
                });
            }
            if function.required_effects.contains("Console.Write")
                && !target
                    .capabilities
                    .iter()
                    .any(|capability| capability == "console.write")
            {
                diagnostics.push(ProjectDiagnostic {
                    path: owner.path.clone(),
                    diagnostic: Diagnostic::error(
                        "RBN4101",
                        "l’effet `Console.Write` exige la capacité manifeste `console.write`",
                        function.span,
                    ),
                });
            }
        } else {
            diagnostics.push(ProjectDiagnostic {
                path: owner.path.clone(),
                diagnostic: Diagnostic::error(
                    "RBN5001",
                    format!("racine `{}` introuvable", target.entry),
                    owner
                        .analysis
                        .program
                        .as_ref()
                        .map_or(Span::default(), |program| program.module_span),
                ),
            });
        }
    } else {
        diagnostics.push(ProjectDiagnostic {
            path: manifest_path.to_path_buf(),
            diagnostic: Diagnostic::error(
                "RBN5001",
                format!("racine `{}` introuvable", target.entry),
                Span::default(),
            ),
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_ORDINAL: AtomicUsize = AtomicUsize::new(0);

    struct TempProject {
        root: PathBuf,
    }

    impl TempProject {
        fn new(name: &str) -> Self {
            let ordinal = TEMP_ORDINAL.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "robine-project-test-{}-{name}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("temporary project root");
            Self { root }
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn project(capabilities: Vec<String>) -> LoadedProject {
        let source = r#"module hello
fn main(console: Console) -> Unit ! { Console.Write } {
    console.write_line("Hello")
}
"#
        .to_owned();
        LoadedProject {
            root: PathBuf::new(),
            manifest_path: PathBuf::from("robine.toml"),
            manifest_source: String::new(),
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
                        source_root: "src".to_owned(),
                        source: "src/main.ro".to_owned(),
                        entry: "hello.main".to_owned(),
                        domain: "normal".to_owned(),
                        capabilities,
                    },
                },
                foreign: BTreeMap::new(),
            },
            source: source.clone(),
            sources: vec![SourceFile {
                path: PathBuf::from("src/main.ro"),
                relative_path: PathBuf::from("src/main.ro"),
                source,
            }],
        }
    }

    fn replace_source(project: &mut LoadedProject, from: &str, to: &str) {
        project.source = project.source.replace(from, to);
        project.sources[0].source = project.source.clone();
    }

    #[test]
    fn accepts_explicit_console_capability() {
        assert!(analyze_project(&project(vec!["console.write".to_owned()])).is_valid());
    }

    #[test]
    fn rejects_missing_console_capability() {
        let result = analyze_project(&project(Vec::new()));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.diagnostic.code == "RBN4101")
        );
    }

    #[test]
    fn entry_capability_parameter_name_is_not_semantic() {
        let mut renamed = project(vec!["console.write".to_owned()]);
        replace_source(&mut renamed, "console", "terminal");
        assert!(analyze_project(&renamed).is_valid());
    }

    #[test]
    fn manifest_selects_the_root_function_by_identity() {
        let mut renamed = project(vec!["console.write".to_owned()]);
        replace_source(&mut renamed, "main", "start");
        renamed.manifest.target.app.entry = "hello.start".to_owned();
        assert!(analyze_project(&renamed).is_valid());
    }

    #[test]
    fn rejects_invalid_package_version_and_application_profile() {
        let mut invalid = project(vec!["console.write".to_owned()]);
        invalid.manifest.package.version = "tomorrow".to_owned();
        invalid.manifest.target.app.profile = "app.future".to_owned();
        let diagnostics = analyze_project(&invalid).diagnostics;
        assert!(
            diagnostics
                .iter()
                .any(|item| item.diagnostic.code == "RBN5006")
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.diagnostic.code == "RBN5007")
        );
    }

    #[test]
    fn rejects_legacy_source_extension() {
        let mut legacy = project(vec!["console.write".to_owned()]);
        legacy.manifest.target.app.source = "src/main.robine".to_owned();
        let diagnostics = analyze_project(&legacy).diagnostics;
        assert!(
            diagnostics
                .iter()
                .any(|item| item.diagnostic.code == "RBN5008")
        );
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
        assert_eq!(manifest.target.app.source_root, "src");
    }

    #[test]
    fn foreign_call_requires_an_exact_manifest_contract() {
        let mut foreign = project(vec!["console.write".to_owned()]);
        replace_source(
            &mut foreign,
            "console.write_line",
            "rust.grapheme_count(\"Robine\");\n    console.write_line",
        );
        let missing = analyze_project(&foreign);
        assert!(
            missing
                .diagnostics
                .iter()
                .any(|item| item.diagnostic.code == "RBN4201")
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

    #[test]
    fn loader_discovers_nested_sources_in_stable_relative_order() {
        let project = TempProject::new("discovery");
        fs::create_dir_all(project.root.join("src/nested")).expect("nested source directory");
        fs::write(
            project.root.join("robine.toml"),
            r#"[package]
name = "discovery"
version = "0.1.0"

[target.app]
source_root = "src"
source = "src/main.ro"
entry = "app.main"
"#,
        )
        .expect("manifest");
        fs::write(
            project.root.join("src/main.ro"),
            "module app\nfn main() -> Unit {}\n",
        )
        .expect("main source");
        fs::write(
            project.root.join("src/nested/math.ro"),
            "module app.math\npub fn answer() -> Int { 42 }\n",
        )
        .expect("nested source");

        let loaded = LoadedProject::load(&project.root).expect("project loads");
        assert_eq!(
            loaded
                .sources
                .iter()
                .map(|file| file.relative_path.as_path())
                .collect::<Vec<_>>(),
            [Path::new("src/main.ro"), Path::new("src/nested/math.ro")]
        );
    }

    #[test]
    fn loader_rejects_source_outside_declared_root() {
        let project = TempProject::new("escape");
        fs::create_dir_all(project.root.join("src")).expect("source directory");
        fs::write(
            project.root.join("robine.toml"),
            r#"[package]
name = "escape"
version = "0.1.0"

[target.app]
source_root = "src"
source = "outside.ro"
entry = "outside.main"
"#,
        )
        .expect("manifest");
        fs::write(project.root.join("outside.ro"), "module outside\n").expect("outside source");
        assert!(
            LoadedProject::load(&project.root)
                .expect_err("outside source must fail")
                .contains("extérieur")
        );
    }

    #[test]
    fn loader_rejects_empty_source_root() {
        let project = TempProject::new("empty");
        fs::create_dir_all(project.root.join("src")).expect("source directory");
        fs::write(
            project.root.join("robine.toml"),
            r#"[package]
name = "empty"
version = "0.1.0"

[target.app]
source_root = "src"
source = "src/main.ro"
entry = "empty.main"
"#,
        )
        .expect("manifest");
        assert!(LoadedProject::load(&project.root).is_err());
    }

    #[test]
    fn source_paths_must_be_project_relative() {
        assert!(safe_relative_path(Path::new("src")));
        assert!(safe_relative_path(Path::new("src/nested")));
        assert!(!safe_relative_path(Path::new("../src")));
        assert!(!safe_relative_path(Path::new("/tmp/src")));
        assert!(!safe_relative_path(Path::new("")));
    }
}
