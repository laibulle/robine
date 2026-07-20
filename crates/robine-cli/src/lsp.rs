use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity as LspSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    DocumentSymbol, DocumentSymbolParams, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, InitializeParams, Location, MarkupContent, MarkupKind, OneOf,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, SymbolKind as LspSymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
};
use robine_core::{Diagnostic, DiagnosticSeverity, Engine, Span, SymbolKind, format_source, parse};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use url::Url;

pub fn serve() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        position_encoding: Some(lsp_types::PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions::default()),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    let initialization = connection
        .initialize(serde_json::to_value(capabilities)?)
        .context("initialisation LSP")?;
    let initialize: InitializeParams =
        serde_json::from_value(initialization).context("paramètres initialize invalides")?;

    let mut server = Server {
        connection: &connection,
        engine: Engine::new(),
        project_root: None,
        disk_sources: BTreeMap::new(),
        published_workspace: false,
    };
    #[allow(deprecated)]
    if let Some(root_uri) = initialize.root_uri {
        server.try_load_workspace_uri(&root_uri)?;
    }
    server.main_loop()?;
    drop(server);
    drop(connection);
    io_threads.join().context("arrêt des flux LSP")?;
    Ok(())
}

struct Server<'a> {
    connection: &'a Connection,
    engine: Engine,
    project_root: Option<PathBuf>,
    disk_sources: BTreeMap<String, String>,
    published_workspace: bool,
}

impl Server<'_> {
    fn main_loop(&mut self) -> Result<()> {
        for message in &self.connection.receiver {
            match message {
                Message::Request(request) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.handle_request(request)?;
                }
                Message::Notification(notification) => {
                    self.handle_notification(notification)?;
                }
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn handle_notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = params.text_document.uri;
                self.try_load_workspace_uri(&uri)?;
                let version = params.text_document.version;
                let source = params.text_document.text;
                self.engine.update(uri.as_str(), version, source);
                if self.published_workspace {
                    self.publish_invalidated(&uri)?;
                } else {
                    self.publish_all()?;
                    self.published_workspace = true;
                }
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                if let Some(change) = params.content_changes.into_iter().last() {
                    let uri = params.text_document.uri;
                    self.engine
                        .update(uri.as_str(), params.text_document.version, change.text);
                    self.publish_invalidated(&uri)?;
                }
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = params.text_document.uri;
                if let Some(source) = self.disk_sources.get(uri.as_str()).cloned() {
                    let version = self
                        .engine
                        .get(uri.as_str())
                        .map_or(0, |snapshot| snapshot.version.saturating_add(1));
                    self.engine.update(uri.as_str(), version, source);
                    self.publish_invalidated(&uri)?;
                } else {
                    self.engine.close(uri.as_str());
                    self.send_notification(
                        "textDocument/publishDiagnostics",
                        PublishDiagnosticsParams::new(uri, Vec::new(), None),
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_request(&self, request: Request) -> Result<()> {
        let result = match request.method.as_str() {
            "textDocument/hover" => self.hover(request.params),
            "textDocument/definition" => self.definition(request.params),
            "textDocument/documentSymbol" => self.document_symbols(request.params),
            "textDocument/completion" => self.completion(request.params),
            "textDocument/formatting" => self.formatting(request.params),
            _ => {
                self.connection
                    .sender
                    .send(Message::Response(Response::new_err(
                        request.id,
                        lsp_server::ErrorCode::MethodNotFound as i32,
                        format!("méthode non prise en charge `{}`", request.method),
                    )))?;
                return Ok(());
            }
        };
        match result {
            Ok(value) => self
                .connection
                .sender
                .send(Message::Response(Response::new_ok(request.id, value)))?,
            Err(error) => self
                .connection
                .sender
                .send(Message::Response(Response::new_err(
                    request.id,
                    lsp_server::ErrorCode::InternalError as i32,
                    format!("{error:#}"),
                )))?,
        }
        Ok(())
    }

    fn hover(&self, params: Value) -> Result<Value> {
        let params: HoverParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let snapshot = self.snapshot(&uri)?;
        let offset = offset_at(
            &snapshot.source,
            params.text_document_position_params.position,
        );
        let hover = snapshot.analysis.symbol_at(offset).map(|symbol| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "**{}** `{}`{}",
                    format!("{:?}", symbol.kind).to_lowercase(),
                    symbol.name,
                    symbol
                        .type_name
                        .as_ref()
                        .map_or_else(String::new, |type_name| format!("\n\n`{type_name}`"))
                ),
            }),
            range: Some(range_for(&snapshot.source, symbol.span)),
        });
        Ok(serde_json::to_value(hover)?)
    }

    fn definition(&self, params: Value) -> Result<Value> {
        let params: GotoDefinitionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let snapshot = self.snapshot(&uri)?;
        let offset = offset_at(
            &snapshot.source,
            params.text_document_position_params.position,
        );
        let response = snapshot.analysis.symbol_at(offset).and_then(|symbol| {
            let definition_uri = symbol
                .definition_module
                .as_deref()
                .and_then(|module| self.engine.uri_for_module(module))
                .and_then(|uri| uri.parse::<Uri>().ok())
                .unwrap_or_else(|| uri.clone());
            let definition_source = self
                .engine
                .get(definition_uri.as_str())
                .map(|snapshot| snapshot.source.as_str())?;
            Some(GotoDefinitionResponse::Scalar(Location::new(
                definition_uri,
                range_for(definition_source, symbol.definition_span),
            )))
        });
        Ok(serde_json::to_value(response)?)
    }

    fn document_symbols(&self, params: Value) -> Result<Value> {
        let params: DocumentSymbolParams = serde_json::from_value(params)?;
        let snapshot = self.snapshot(&params.text_document.uri)?;
        #[allow(deprecated)]
        let symbols = snapshot
            .analysis
            .symbols
            .iter()
            .filter(|symbol| {
                matches!(symbol.kind, SymbolKind::Module | SymbolKind::Function)
                    && symbol.span == symbol.definition_span
            })
            .map(|symbol| DocumentSymbol {
                name: symbol.name.clone(),
                detail: symbol.type_name.clone(),
                kind: match symbol.kind {
                    SymbolKind::Module => LspSymbolKind::MODULE,
                    _ => LspSymbolKind::FUNCTION,
                },
                tags: None,
                deprecated: None,
                range: range_for(&snapshot.source, symbol.span),
                selection_range: range_for(&snapshot.source, symbol.span),
                children: None,
            })
            .collect::<Vec<_>>();
        Ok(serde_json::to_value(symbols)?)
    }

    fn completion(&self, params: Value) -> Result<Value> {
        let params: CompletionParams = serde_json::from_value(params)?;
        let snapshot = self.snapshot(&params.text_document_position.text_document.uri)?;
        let mut items = [
            "module", "import", "pub", "fn", "let", "if", "else", "true", "false",
        ]
        .into_iter()
        .map(|keyword| CompletionItem {
            label: keyword.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect::<Vec<_>>();
        items.extend(snapshot.analysis.symbols.iter().filter_map(|symbol| {
            if symbol.span != symbol.definition_span {
                return None;
            }
            Some(CompletionItem {
                label: symbol.name.clone(),
                kind: Some(match symbol.kind {
                    SymbolKind::Function => CompletionItemKind::FUNCTION,
                    SymbolKind::Module => CompletionItemKind::MODULE,
                    SymbolKind::Parameter | SymbolKind::Local => CompletionItemKind::VARIABLE,
                    SymbolKind::Type => CompletionItemKind::CLASS,
                    SymbolKind::Effect => CompletionItemKind::ENUM_MEMBER,
                }),
                detail: symbol.type_name.clone(),
                ..CompletionItem::default()
            })
        }));
        if let Some(program) = &snapshot.analysis.program {
            for import in &program.imports {
                items.push(CompletionItem {
                    label: import.module.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    ..CompletionItem::default()
                });
                if let Some(provider_uri) = self.engine.uri_for_module(&import.module)
                    && let Some(provider) = self.engine.get(provider_uri)
                {
                    items.extend(
                        provider
                            .analysis
                            .functions
                            .iter()
                            .filter(|function| function.public)
                            .map(|function| CompletionItem {
                                label: function.qualified_name.clone(),
                                kind: Some(CompletionItemKind::FUNCTION),
                                detail: Some(function.return_type.to_string()),
                                ..CompletionItem::default()
                            }),
                    );
                }
            }
        }
        Ok(serde_json::to_value(CompletionResponse::Array(items))?)
    }

    fn formatting(&self, params: Value) -> Result<Value> {
        let params: DocumentFormattingParams = serde_json::from_value(params)?;
        let snapshot = self.snapshot(&params.text_document.uri)?;
        let edits = match parse(&snapshot.source) {
            Ok(program) => vec![TextEdit {
                range: full_range(&snapshot.source),
                new_text: format_source(&snapshot.source, &program),
            }],
            Err(_) => Vec::new(),
        };
        Ok(serde_json::to_value(edits)?)
    }

    fn snapshot(&self, uri: &Uri) -> Result<&robine_core::DocumentSnapshot> {
        self.engine
            .get(uri.as_str())
            .with_context(|| format!("document `{}` non ouvert", uri.as_str()))
    }

    fn publish(&self, uri: &Uri) -> Result<()> {
        let snapshot = self.snapshot(uri)?;
        let diagnostics = snapshot
            .analysis
            .diagnostics
            .iter()
            .map(|diagnostic| to_lsp_diagnostic(&snapshot.source, diagnostic))
            .collect();
        self.send_notification(
            "textDocument/publishDiagnostics",
            PublishDiagnosticsParams::new(uri.clone(), diagnostics, Some(snapshot.version)),
        )
    }

    fn publish_all(&self) -> Result<()> {
        let uris = self
            .engine
            .snapshots()
            .filter_map(|(uri, _)| uri.parse::<Uri>().ok())
            .collect::<Vec<_>>();
        for uri in uris {
            self.publish(&uri)?;
        }
        Ok(())
    }

    fn publish_invalidated(&self, changed_uri: &Uri) -> Result<()> {
        let mut uris = self
            .engine
            .last_invalidation()
            .retyped
            .iter()
            .filter_map(|module| {
                self.engine
                    .uri_for_module(module)
                    .or_else(|| self.engine.get(module).map(|_| module.as_str()))
            })
            .filter_map(|uri| uri.parse::<Uri>().ok())
            .collect::<Vec<_>>();
        if !uris.iter().any(|uri| uri == changed_uri) {
            uris.push(changed_uri.clone());
        }
        uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        uris.dedup();
        for uri in uris {
            self.publish(&uri)?;
        }
        Ok(())
    }

    fn try_load_workspace_uri(&mut self, uri: &Uri) -> Result<()> {
        let Ok(url) = Url::parse(uri.as_str()) else {
            return Ok(());
        };
        let Ok(path) = url.to_file_path() else {
            return Ok(());
        };
        let Some(root) = find_project_root(&path) else {
            return Ok(());
        };
        if self.project_root.as_deref() == Some(root.as_path()) {
            return Ok(());
        }
        let project = robine_core::LoadedProject::load(&root).map_err(anyhow::Error::msg)?;
        self.disk_sources.clear();
        for file in project.sources {
            let source_uri = file_uri(&file.path)?;
            self.disk_sources
                .insert(source_uri.as_str().to_owned(), file.source.clone());
            self.engine.update(source_uri.as_str(), 0, file.source);
        }
        self.project_root = Some(root);
        self.published_workspace = false;
        Ok(())
    }

    fn send_notification(&self, method: &'static str, params: impl serde::Serialize) -> Result<()> {
        self.connection
            .sender
            .send(Message::Notification(Notification::new(
                method.to_owned(),
                params,
            )))?;
        Ok(())
    }
}

fn find_project_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    start
        .ancestors()
        .find(|ancestor| ancestor.join("robine.toml").is_file())
        .map(Path::to_path_buf)
}

fn file_uri(path: &Path) -> Result<Uri> {
    let url = Url::from_file_path(path)
        .map_err(|()| anyhow::anyhow!("chemin non convertible en URI: {}", path.display()))?;
    url.as_str()
        .parse()
        .map_err(|error| anyhow::anyhow!("URI de source invalide: {error}"))
}

fn to_lsp_diagnostic(source: &str, diagnostic: &Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: range_for(source, diagnostic.span),
        severity: Some(match diagnostic.severity {
            DiagnosticSeverity::Error => LspSeverity::ERROR,
            DiagnosticSeverity::Warning => LspSeverity::WARNING,
        }),
        code: Some(lsp_types::NumberOrString::String(
            diagnostic.code.to_owned(),
        )),
        code_description: None,
        source: Some("robine".to_owned()),
        message: diagnostic.message.clone(),
        related_information: None,
        tags: None,
        data: Some(serde_json::json!({
            "syntaxProfile": robine_core::SYNTAX_PROFILE,
        })),
    }
}

fn range_for(source: &str, span: Span) -> Range {
    Range::new(
        position_at(source, span.start),
        position_at(source, span.end),
    )
}

fn full_range(source: &str) -> Range {
    Range::new(Position::new(0, 0), position_at(source, source.len()))
}

fn position_at(source: &str, offset: usize) -> Position {
    let safe_offset = floor_char_boundary(source, offset.min(source.len()));
    let before = &source[..safe_offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let character = source[line_start..safe_offset].encode_utf16().count();
    Position::new(
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(character).unwrap_or(u32::MAX),
    )
}

fn offset_at(source: &str, position: Position) -> usize {
    let line_start = if position.line == 0 {
        0
    } else {
        source
            .match_indices('\n')
            .nth(position.line.saturating_sub(1) as usize)
            .map_or(source.len(), |(index, _)| index + 1)
    };
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |relative| line_start + relative);
    let line = &source[line_start..line_end];
    let mut utf16 = 0_u32;
    for (byte_offset, ch) in line.char_indices() {
        if utf16 >= position.character {
            return line_start + byte_offset;
        }
        utf16 += u32::try_from(ch.len_utf16()).unwrap_or(2);
        if utf16 > position.character {
            return line_start + byte_offset;
        }
    }
    line_end
}

fn floor_char_boundary(source: &str, mut offset: usize) -> usize {
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_handle_non_bmp_text() {
        let source = "a👩🏽‍💻b\nx";
        let byte_of_b = source.find('b').expect("b");
        assert_eq!(position_at(source, byte_of_b), Position::new(0, 8));
        assert_eq!(offset_at(source, Position::new(0, 8)), byte_of_b);
    }

    #[test]
    fn stale_engine_snapshots_remain_newest() {
        let uri = "file:///main.ro".parse::<Uri>().expect("URI");
        let mut engine = Engine::new();
        engine.update(uri.as_str(), 4, "module newest");
        engine.update(uri.as_str(), 3, "module stale");
        assert_eq!(engine.get(uri.as_str()).expect("snapshot").version, 4);
    }
}
