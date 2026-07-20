use zed_extension_api::{self as zed, Result};

struct RobineExtension;

impl zed::Extension for RobineExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let command = worktree.which("robine").ok_or_else(|| {
            "robine executable not found in $PATH; run `cargo install --path crates/robine-cli`"
                .to_owned()
        })?;
        Ok(zed::Command {
            command,
            args: vec!["lsp".to_owned(), "--stdio".to_owned()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(RobineExtension);
