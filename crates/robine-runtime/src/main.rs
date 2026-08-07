use std::{env, path::PathBuf, sync::Arc};

use actix_web::{App, HttpServer, web};
use anyhow::Context;
use robine_api_http::{NoopCommandDispatcher, ServerState, configure as configure_api};
use robine_application::HomeService;
use robine_mcp_http::{
    AuthenticationError, McpAuthenticator, McpHttpState, configure as configure_mcp,
};
use robine_mcp_tools::McpTools;
use robine_mcp_types::{Scope, Scopes};
use robine_store_sqlite::SqliteStore;

struct RuntimeMcpAuthenticator {
    store: Arc<SqliteStore>,
}

impl McpAuthenticator for RuntimeMcpAuthenticator {
    fn authenticate(&self, bearer: &str) -> Result<Scopes, AuthenticationError> {
        self.store
            .authenticate(bearer)
            .ok()
            .filter(|valid| *valid)
            .map(|_| Scopes::new([Scope::Read]))
            .ok_or(AuthenticationError::Invalid)
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let data_dir = env::var_os("ROBINE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));
    std::fs::create_dir_all(&data_dir).context("creating Robine data directory")?;
    let store = Arc::new(SqliteStore::open(data_dir.join("robine.sqlite3"))?);
    let dispatcher = Arc::new(NoopCommandDispatcher);
    let service = HomeService::new(store.clone(), store.clone(), dispatcher);
    let bind = env::var("ROBINE_BIND").unwrap_or_else(|_| "127.0.0.1:3030".into());
    let api_state = ServerState::new(service.clone(), store.clone());
    let mcp_state = McpHttpState::new(
        McpTools::new(service),
        Arc::new(RuntimeMcpAuthenticator { store }),
        std::iter::empty(),
    );
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(api_state.clone()))
            .configure(configure_api)
            .configure(|configuration| configure_mcp(configuration, mcp_state.clone()))
    })
    .bind(&bind)?
    .run()
    .await?;
    Ok(())
}
