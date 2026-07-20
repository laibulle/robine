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
    let _: InitializeParams =
        serde_json::from_value(initialization).context("paramètres initialize invalides")?;

    let mut server = Server {
        connection: &connection,
        engine: Engine::new(),
    };
    server.main_loop()?;
    drop(server);
    drop(connection);
    io_threads.join().context("arrêt des flux LSP")?;
    Ok(())
}

struct Server<'a> {
    connection: &'a Connection,
    engine: Engine,
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
                let version = params.text_document.version;
                let source = params.text_document.text;
                self.engine.update(uri.as_str(), version, source);
                self.publish(&uri)?;
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                if let Some(change) = params.content_changes.into_iter().last() {
                    let uri = params.text_document.uri;
                    self.engine
                        .update(uri.as_str(), params.text_document.version, change.text);
                    self.publish(&uri)?;
                }
            }
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.engine.close(params.text_document.uri.as_str());
                self.send_notification(
                    "textDocument/publishDiagnostics",
                    PublishDiagnosticsParams::new(params.text_document.uri, Vec::new(), None),
                )?;
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
        let response = snapshot.analysis.symbol_at(offset).map(|symbol| {
            GotoDefinitionResponse::Scalar(Location::new(
                uri,
                range_for(&snapshot.source, symbol.definition_span),
            ))
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
        let mut items = ["module", "fn", "let", "true", "false"]
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
        let uri = "file:///main.robine".parse::<Uri>().expect("URI");
        let mut engine = Engine::new();
        engine.update(uri.as_str(), 4, "module newest");
        engine.update(uri.as_str(), 3, "module stale");
        assert_eq!(engine.get(uri.as_str()).expect("snapshot").version, 4);
    }
}
