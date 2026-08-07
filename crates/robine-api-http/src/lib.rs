//! Adaptateur HTTP Actix : il traduit le contrat réseau vers les cas d'utilisation.

use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer,
    cookie::{Cookie, SameSite, time::Duration as CookieDuration},
    http::{StatusCode, header},
    web,
};
use actix_ws::Message;
use chrono::Utc;
use futures_util::StreamExt;
use robine_api_contract::*;
use robine_application::{
    ApplicationError, CommandDispatcher, DevicePageRequest, FlowError, FlowService, HomeService,
};
use robine_domain::{Area, AreaId, Command, DeviceId, DeviceStatus, EntityId, FlowId};
use robine_matter_contract::CommissioningJob;
use robine_store_sqlite::SqliteStore;
use std::sync::Arc;

pub trait HueAdministration: Send + Sync {
    fn discover(&self) -> Result<Vec<HueBridgeCandidate>, HueAdministrationError>;
    fn pair(&self, request: HuePairRequest) -> Result<HuePairResponse, HueAdministrationError>;
    fn synchronize(&self) -> Result<usize, HueAdministrationError>;
    fn room_suggestions(&self) -> Result<Vec<HueRoomSuggestion>, HueAdministrationError>;
    fn import_room(&self, suggestion: HueRoomSuggestion) -> Result<Area, HueAdministrationError>;
}

#[derive(Debug)]
pub enum HueAdministrationError {
    ButtonNotPressed,
    InvalidRequest(String),
    Unavailable(String),
}

pub trait MatterAdministration: Send + Sync {
    fn start_commissioning(&self, setup_code: String) -> Result<String, MatterAdministrationError>;
    fn commissioning_job(
        &self,
        job_id: String,
    ) -> Result<CommissioningJob, MatterAdministrationError>;
}

pub trait BackupAdministration: Send + Sync {
    fn create_backup(&self) -> Result<BackupResponse, ApplicationError>;
}

#[derive(Debug)]
pub enum MatterAdministrationError {
    Unavailable,
    JobNotFound,
}

#[derive(Clone)]
pub struct ServerState {
    pub service: HomeService,
    pub flows: FlowService,
    pub store: Arc<SqliteStore>,
    pub hue: Option<Arc<dyn HueAdministration>>,
    pub matter: Option<Arc<dyn MatterAdministration>>,
    pub backups: Option<Arc<dyn BackupAdministration>>,
}
impl ServerState {
    pub fn new(service: HomeService, store: Arc<SqliteStore>) -> Self {
        Self {
            flows: FlowService::new(store.clone(), store.clone()),
            service,
            store,
            hue: None,
            matter: None,
            backups: None,
        }
    }
    pub fn with_hue(mut self, hue: Arc<dyn HueAdministration>) -> Self {
        self.hue = Some(hue);
        self
    }
    pub fn with_matter(mut self, matter: Arc<dyn MatterAdministration>) -> Self {
        self.matter = Some(matter);
        self
    }
    pub fn with_backups(mut self, backups: Arc<dyn BackupAdministration>) -> Self {
        self.backups = Some(backups);
        self
    }
}

/// Dispatcher de démarrage : les intégrations le remplacent avant de déclarer
/// leurs entités disponibles.
pub struct NoopCommandDispatcher;
impl CommandDispatcher for NoopCommandDispatcher {
    fn dispatch(&self, _command: Command) -> Result<(), ApplicationError> {
        Ok(())
    }
}

pub async fn run(bind: &str, state: ServerState) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .configure(configure)
    })
    .bind(bind)?
    .run()
    .await
}

pub fn configure(configuration: &mut web::ServiceConfig) {
    configuration
        .route("/health", web::get().to(health))
        .service(
            web::scope("/api/v1")
                .route("/openapi.json", web::get().to(openapi))
                .route(
                    "/setup/administrator",
                    web::post().to(bootstrap_administrator),
                )
                .route("/auth/tokens", web::post().to(issue_token))
                .route("/auth/stream-session", web::post().to(issue_stream_session))
                .route("/auth/mcp-tokens", web::post().to(issue_mcp_token))
                .route("/auth/mcp-approvals", web::post().to(create_mcp_approval))
                .route("/devices", web::get().to(list_devices))
                .route("/devices/{id}", web::patch().to(rename_device))
                .route("/devices/{id}", web::delete().to(remove_device))
                .route("/entities/{id}", web::get().to(entity_detail))
                .route("/entities/{id}", web::patch().to(rename_entity))
                .route("/entities/{id}/area", web::put().to(assign_entity_area))
                .route("/entities/{id}/commands", web::post().to(request_command))
                .route("/areas", web::get().to(list_areas))
                .route("/areas", web::post().to(create_area))
                .route("/adapters", web::get().to(list_adapters))
                .route("/adapters/hue/discover", web::get().to(discover_hue))
                .route("/adapters/hue/pair", web::post().to(pair_hue))
                .route("/adapters/hue/synchronize", web::post().to(synchronize_hue))
                .route("/adapters/hue/rooms", web::get().to(hue_room_suggestions))
                .route(
                    "/adapters/hue/rooms/import",
                    web::post().to(import_hue_room),
                )
                .route(
                    "/adapters/matter/commission",
                    web::post().to(commission_matter),
                )
                .route("/adapters/matter/jobs/{id}", web::get().to(matter_job))
                .route("/backups", web::post().to(create_backup))
                .route("/automations", web::get().to(list_automations))
                .route("/automations", web::post().to(create_automation))
                .route(
                    "/automations/{id}/runs",
                    web::get().to(list_automation_runs),
                )
                .route("/automations/{id}", web::patch().to(update_automation))
                .route(
                    "/automations/{id}/simulate",
                    web::post().to(simulate_automation),
                )
                .route("/events", web::get().to(events))
                .route("/stream", web::get().to(stream)),
        );
}

async fn openapi() -> HttpResponse {
    HttpResponse::Ok().json(robine_api_contract::openapi_document())
}

async fn health(state: web::Data<ServerState>) -> HttpResponse {
    let store = state.store.clone();
    let service = state.service.clone();
    match blocking(move || {
        let initialized = store.is_initialized()?;
        let adapters = service.list_adapter_health()?;
        let degraded = adapters.iter().any(|adapter| {
            matches!(
                adapter.status,
                robine_domain::AdapterStatus::Degraded | robine_domain::AdapterStatus::Unauthorized
            )
        });
        Ok(serde_json::json!({
            "status": if degraded { "degraded" } else { "healthy" },
            "initialized": initialized,
            "degraded_adapters": adapters.into_iter().filter(|adapter| matches!(adapter.status, robine_domain::AdapterStatus::Degraded | robine_domain::AdapterStatus::Unauthorized)).map(|adapter| adapter.adapter_id.0).collect::<Vec<_>>()
        }))
    })
    .await
    {
        Ok(health) => HttpResponse::Ok().json(health),
        Err(response) => response,
    }
}

async fn bootstrap_administrator(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<BootstrapAdministratorRequest>,
) -> HttpResponse {
    if !request_is_loopback(&request) {
        return error_response(
            StatusCode::FORBIDDEN,
            "setup_loopback_only",
            "initial setup is only available on loopback",
        );
    }
    let password = body.into_inner().password;
    let store = state.store.clone();
    match blocking(move || store.bootstrap_administrator(&password, Utc::now())).await {
        Ok(token) => HttpResponse::build(StatusCode::CREATED).json(TokenResponse { token }),
        Err(response) => response,
    }
}

async fn issue_token(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<IssueTokenRequest>,
) -> HttpResponse {
    // La machine qui héberge Robine peut récupérer une association perdue avec
    // le mot de passe administrateur. Depuis le LAN, un bearer existant reste
    // obligatoire : le mot de passe seul ne devient jamais une API de login
    // exposée au réseau domestique.
    if !request_is_loopback(&request) {
        if let Err(response) = authorize(&request, &state).await {
            return response;
        }
    }
    let password = body.into_inner().password;
    let store = state.store.clone();
    match blocking(move || store.issue_token(&password, Utc::now())).await {
        Ok(token) => HttpResponse::build(StatusCode::CREATED).json(TokenResponse { token }),
        Err(response) => response,
    }
}

/// Les navigateurs ne peuvent pas joindre un header Authorization à un
/// handshake WebSocket. Le bearer n'est donc jamais mis dans l'URL : cette
/// courte session HttpOnly est strictement limitée au flux produit.
async fn issue_stream_session(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    let Some(bearer) = bearer_from_header(&request) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a local bearer token is required",
        );
    };
    if let Err(response) = authorize_bearer(bearer.clone(), &state).await {
        return response;
    }
    let secure = request.connection_info().scheme() == "https";
    let cookie = Cookie::build("robine_stream", bearer)
        .path("/api/v1/stream")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .max_age(CookieDuration::minutes(10))
        .finish();
    HttpResponse::NoContent().cookie(cookie).finish()
}

fn request_is_loopback(request: &HttpRequest) -> bool {
    request
        .peer_addr()
        .is_some_and(|address| address.ip().is_loopback())
}

async fn issue_mcp_token(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<IssueMcpTokenRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let request = body.into_inner();
    let expiry = request.expires_in_seconds.unwrap_or(86_400);
    let scopes = request.scopes.unwrap_or_else(|| vec!["read".into()]);
    let write_policy = request.write_policy;
    let store = state.store.clone();
    match blocking(move || store.issue_mcp_token(&scopes, write_policy, expiry, Utc::now())).await {
        Ok((token, identity, expires_at)) => HttpResponse::Created().json(McpTokenResponse {
            token,
            token_id: identity.token_id,
            expires_at: expires_at.to_rfc3339(),
            scopes: identity
                .scopes
                .into_iter()
                .map(|scope| format!("robine:{scope}"))
                .collect(),
        }),
        Err(response) => response,
    }
}

async fn create_mcp_approval(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<CreateMcpApprovalRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let body = body.into_inner();
    if !body.arguments.is_object() || body.arguments.get("approval_id").is_some() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_mcp_approval",
            "approval arguments must be an object without approval_id",
        );
    }
    let arguments_hash = robine_mcp_types::approval_arguments_hash(&body.arguments);
    let expiry = body.expires_in_seconds.unwrap_or(300);
    let store = state.store.clone();
    match blocking(move || {
        store.create_mcp_approval(
            &body.token_id,
            &body.tool,
            &arguments_hash,
            expiry,
            Utc::now(),
        )
    })
    .await
    {
        Ok((approval_id, expires_at)) => HttpResponse::Created().json(McpApprovalResponse {
            approval_id,
            expires_at: expires_at.to_rfc3339(),
        }),
        Err(response) => response,
    }
}

#[derive(Debug, serde::Deserialize)]
struct DeviceListQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    status: Option<String>,
}

async fn list_devices(
    request: HttpRequest,
    state: web::Data<ServerState>,
    query: web::Query<DeviceListQuery>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let query = query.into_inner();
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_device_page",
            "limit must be between 1 and 100",
        );
    }
    let status = match query.status.as_deref() {
        None => None,
        Some("discovered") => Some(DeviceStatus::Discovered),
        Some("available") => Some(DeviceStatus::Available),
        Some("unavailable") => Some(DeviceStatus::Unavailable),
        Some("removed") => Some(DeviceStatus::Removed),
        Some(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_device_page",
                "status must be discovered, available, unavailable, or removed",
            );
        }
    };
    let cursor = match query.cursor {
        Some(cursor) => match uuid::Uuid::parse_str(&cursor) {
            Ok(cursor) => Some(DeviceId(cursor)),
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_device_page",
                    "cursor must be a device UUID",
                );
            }
        },
        None => None,
    };
    let service = state.service.clone();
    match blocking(move || {
        let page = service.list_devices_page(DevicePageRequest {
            cursor,
            limit,
            status,
        })?;
        Ok::<_, ApplicationError>(DevicePage {
            devices: page.devices,
            next_cursor: page.next_cursor.map(|cursor| cursor.to_string()),
        })
    })
    .await
    {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(response) => response,
    }
}

async fn entity_detail(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_entity_id",
            "entity id must be a UUID",
        );
    };
    let service = state.service.clone();
    match blocking(move || service.entity_detail(&EntityId(id))).await {
        Ok(Some(detail)) => HttpResponse::Ok().json(detail),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "entity_not_found",
            "entity not found",
        ),
        Err(response) => response,
    }
}

async fn rename_device(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    body: web::Json<RenameRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_device_id",
            "device id must be a UUID",
        );
    };
    let name = body.into_inner().name;
    let service = state.service.clone();
    match blocking(move || service.rename_device(DeviceId(id), name, Utc::now())).await {
        Ok(device) => HttpResponse::Ok().json(device),
        Err(response) => response,
    }
}

async fn remove_device(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_device_id",
            "device id must be a UUID",
        );
    };
    let service = state.service.clone();
    match blocking(move || service.remove_device(DeviceId(id), Utc::now())).await {
        Ok(device) => HttpResponse::Ok().json(device),
        Err(response) => response,
    }
}

async fn rename_entity(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    body: web::Json<RenameRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_entity_id",
            "entity id must be a UUID",
        );
    };
    let name = body.into_inner().name;
    let service = state.service.clone();
    match blocking(move || service.rename_entity(EntityId(id), name, Utc::now())).await {
        Ok(entity) => HttpResponse::Ok().json(entity),
        Err(response) => response,
    }
}

async fn request_command(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    body: web::Json<CommandRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(idempotency_key) = request
        .headers()
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Idempotency-Key is required",
        );
    };
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_entity_id",
            "entity id must be a UUID",
        );
    };
    let command = body.into_inner();
    let service = state.service.clone();
    match blocking(move || {
        service.request_command(
            EntityId(id),
            command.key,
            command.value,
            idempotency_key,
            Utc::now(),
        )
    })
    .await
    {
        Ok(command) => HttpResponse::build(StatusCode::ACCEPTED).json(CommandAccepted {
            command_id: command.id,
            correlation_id: command.correlation_id,
        }),
        Err(response) => response,
    }
}

async fn assign_entity_area(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    body: web::Json<AssignEntityAreaRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_entity_id",
            "entity id must be a UUID",
        );
    };
    let area_id = body.into_inner().area_id.map(AreaId);
    let service = state.service.clone();
    match blocking(move || service.assign_entity_area(EntityId(id), area_id, Utc::now())).await {
        Ok(entity) => HttpResponse::Ok().json(entity),
        Err(response) => response,
    }
}

async fn list_areas(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let service = state.service.clone();
    match blocking(move || service.list_areas()).await {
        Ok(areas) => HttpResponse::Ok().json(areas),
        Err(response) => response,
    }
}

async fn create_area(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<CreateAreaRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let name = body.into_inner().name;
    let service = state.service.clone();
    match blocking(move || service.create_area(name, Utc::now())).await {
        Ok(area) => HttpResponse::build(StatusCode::CREATED).json(area),
        Err(response) => response,
    }
}

async fn list_adapters(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let service = state.service.clone();
    match blocking(move || service.list_adapter_health()).await {
        Ok(adapters) => HttpResponse::Ok().json(adapters),
        Err(response) => response,
    }
}

async fn discover_hue(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(hue) = state.hue.clone() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "hue_unavailable",
            "Hue administration is not configured",
        );
    };
    match web::block(move || hue.discover()).await {
        Ok(Ok(candidates)) => HttpResponse::Ok().json(candidates),
        Ok(Err(error)) => hue_error(error),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        ),
    }
}

async fn pair_hue(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<HuePairRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(hue) = state.hue.clone() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "hue_unavailable",
            "Hue administration is not configured",
        );
    };
    let request = body.into_inner();
    match web::block(move || hue.pair(request)).await {
        Ok(Ok(response)) => HttpResponse::Created().json(response),
        Ok(Err(error)) => hue_error(error),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        ),
    }
}

async fn synchronize_hue(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(hue) = state.hue.clone() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "hue_unavailable",
            "Hue administration is not configured",
        );
    };
    match web::block(move || hue.synchronize()).await {
        Ok(Ok(discovered_devices)) => {
            HttpResponse::Ok().json(serde_json::json!({ "discovered_devices": discovered_devices }))
        }
        Ok(Err(error)) => hue_error(error),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        ),
    }
}

async fn hue_room_suggestions(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(hue) = state.hue.clone() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "hue_unavailable",
            "Hue administration is not configured",
        );
    };
    match web::block(move || hue.room_suggestions()).await {
        Ok(Ok(suggestions)) => HttpResponse::Ok().json(suggestions),
        Ok(Err(error)) => hue_error(error),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        ),
    }
}

async fn import_hue_room(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<HueRoomImportRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(hue) = state.hue.clone() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "hue_unavailable",
            "Hue administration is not configured",
        );
    };
    match web::block(move || hue.import_room(body.into_inner().suggestion)).await {
        Ok(Ok(area)) => HttpResponse::Created().json(area),
        Ok(Err(error)) => hue_error(error),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        ),
    }
}

async fn commission_matter(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<MatterCommissionRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let setup_code = body.into_inner().setup_code;
    if setup_code.trim().is_empty() || setup_code.len() > 256 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_matter_setup_code",
            "Matter setup code is invalid",
        );
    }
    let Some(matter) = state.matter.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "matter_unavailable",
            "Matter controller is not configured",
        );
    };
    match blocking_matter(move || matter.start_commissioning(setup_code)).await {
        Ok(job_id) => HttpResponse::Accepted().json(MatterCommissionResponse { job_id }),
        Err(response) => response,
    }
}

async fn matter_job(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(matter) = state.matter.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "matter_unavailable",
            "Matter controller is not configured",
        );
    };
    let job_id = path.into_inner();
    match blocking_matter(move || matter.commissioning_job(job_id)).await {
        Ok(job) => HttpResponse::Ok().json(MatterCommissionJobResponse { job }),
        Err(response) => response,
    }
}

async fn create_backup(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Some(backups) = state.backups.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            "Backup administration is not configured",
        );
    };
    match blocking(move || backups.create_backup()).await {
        Ok(backup) => HttpResponse::Created().json(backup),
        Err(response) => response,
    }
}

async fn list_automations(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let flows = state.flows.clone();
    match blocking_flow(move || flows.list()).await {
        Ok(flows) => HttpResponse::Ok().json(flows),
        Err(response) => response,
    }
}

async fn create_automation(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<FlowUpsertRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let request = body.into_inner();
    let flows = state.flows.clone();
    match blocking_flow(move || flows.create(request.source, request.enabled, Utc::now())).await {
        Ok(flow) => HttpResponse::build(StatusCode::CREATED).json(flow),
        Err(response) => response,
    }
}

async fn update_automation(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    body: web::Json<FlowUpsertRequest>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_flow_id",
            "automation id must be a UUID",
        );
    };
    let request = body.into_inner();
    let flows = state.flows.clone();
    match blocking_flow(move || {
        flows.update(FlowId(id), request.source, request.enabled, Utc::now())
    })
    .await
    {
        Ok(flow) => HttpResponse::Ok().json(flow),
        Err(response) => response,
    }
}

#[derive(Debug, serde::Deserialize)]
struct RunHistoryQuery {
    limit: Option<usize>,
}

async fn list_automation_runs(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
    query: web::Query<RunHistoryQuery>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_flow_id",
            "automation id must be a UUID",
        );
    };
    let limit = query.limit.unwrap_or(20);
    let flows = state.flows.clone();
    match blocking_flow(move || flows.list_runs(&FlowId(id), limit)).await {
        Ok(runs) => HttpResponse::Ok().json(runs),
        Err(response) => response,
    }
}

async fn simulate_automation(
    request: HttpRequest,
    state: web::Data<ServerState>,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let Ok(id) = uuid::Uuid::parse_str(&path.into_inner()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_flow_id",
            "automation id must be a UUID",
        );
    };
    let flows = state.flows.clone();
    match blocking_flow(move || flows.simulate_existing(&FlowId(id))).await {
        Ok(simulation) => HttpResponse::Ok().json(simulation),
        Err(response) => response,
    }
}

async fn events(
    request: HttpRequest,
    state: web::Data<ServerState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    if let Some(tail) = query.get("tail") {
        if query.contains_key("after") {
            return error_response(
                StatusCode::BAD_REQUEST,
                "ambiguous_event_query",
                "tail and after cannot be used together",
            );
        }
        let limit = match tail.parse::<usize>() {
            Ok(value @ 1..=500) => value,
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_tail",
                    "tail must be between 1 and 500",
                );
            }
        };
        let service = state.service.clone();
        return match blocking(move || service.recent_events(limit)).await {
            Ok(events) => {
                let next_cursor = events.last().map(|event| event.sequence).unwrap_or(0);
                HttpResponse::Ok().json(EventPage {
                    events: events.into_iter().map(Into::into).collect(),
                    next_cursor,
                })
            }
            Err(response) => response,
        };
    }
    let after = match query.get("after") {
        Some(value) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_cursor",
                    "after must be an unsigned integer",
                );
            }
        },
        None => 0,
    };
    let limit = match query.get("limit") {
        Some(value) => match value.parse::<usize>() {
            Ok(value @ 1..=500) => value,
            _ => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_limit",
                    "limit must be between 1 and 500",
                );
            }
        },
        None => 500,
    };
    let service = state.service.clone();
    match blocking(
        move || -> Result<Option<Vec<robine_domain::EventEnvelope>>, ApplicationError> {
            let earliest = service.earliest_event_sequence()?;
            let latest = service.latest_event_sequence()?;
            if after > latest || cursor_is_outside_retention(after, earliest) {
                return Ok(None);
            }
            service.events_after(after, limit).map(Some)
        },
    )
    .await
    {
        Ok(Some(events)) => {
            let next_cursor = events.last().map(|event| event.sequence).unwrap_or(after);
            HttpResponse::Ok().json(EventPage {
                events: events.into_iter().map(Into::into).collect(),
                next_cursor,
            })
        }
        Ok(None) => error_response(
            StatusCode::CONFLICT,
            "resync_required",
            "event cursor is outside the retained journal",
        ),
        Err(response) => response,
    }
}

async fn stream(
    request: HttpRequest,
    body: web::Payload,
    state: web::Data<ServerState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Err(response) = authorize_stream(&request, &state).await {
        return Ok(response);
    }
    let (response, mut session, mut messages) = actix_ws::handle(&request, body)?;
    let service = state.service.clone();
    let store = state.store.clone();
    actix_web::rt::spawn(async move {
        let Some(Ok(Message::Text(text))) = messages.next().await else {
            return;
        };
        let Ok(StreamClientMessage::Subscribe { topics, after }) = serde_json::from_str(&text)
        else {
            let _ = session.close(None).await;
            return;
        };
        if topics.is_empty() || topics.iter().any(|topic| !is_stream_topic(topic)) {
            let _ = session.close(None).await;
            return;
        }
        let mut receiver = store.subscribe_events();
        let after = after.unwrap_or(0);
        let latest = service.latest_event_sequence().unwrap_or(0);
        let earliest = service.earliest_event_sequence().unwrap_or(None);
        if after > latest || cursor_is_outside_retention(after, earliest) {
            let _ = send_stream(&mut session, StreamServerMessage::ResyncRequired).await;
            let _ = session.close(None).await;
            return;
        }
        let replay = service.events_after(after, 501).unwrap_or_default();
        if replay.len() > 500 {
            let _ = send_stream(&mut session, StreamServerMessage::ResyncRequired).await;
            let _ = session.close(None).await;
            return;
        }
        let cursor = replay.last().map(|event| event.sequence).unwrap_or(after);
        if send_stream(&mut session, StreamServerMessage::Ready { cursor })
            .await
            .is_err()
        {
            return;
        }
        for event in replay
            .into_iter()
            .filter(|event| topics.iter().any(|topic| topic == event.data.topic()))
        {
            if send_stream(
                &mut session,
                StreamServerMessage::Event {
                    event: event.into(),
                },
            )
            .await
            .is_err()
            {
                return;
            }
        }
        let (outbound_sender, mut outbound_receiver) = tokio::sync::mpsc::channel(128);
        let (overflow_sender, mut overflow_receiver) = tokio::sync::oneshot::channel();
        let producer_topics = topics.clone();
        let producer = actix_web::rt::spawn(async move {
            let mut overflow_sender = Some(overflow_sender);
            loop {
                match receiver.recv().await {
                    Ok(event)
                        if event.sequence > cursor
                            && producer_topics
                                .iter()
                                .any(|topic| topic == event.data.topic()) =>
                    {
                        if outbound_sender.try_send(event).is_err() {
                            let _ = overflow_sender
                                .take()
                                .expect("overflow sender is available")
                                .send(());
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = overflow_sender
                            .take()
                            .expect("overflow sender is available")
                            .send(());
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        loop {
            tokio::select! {
                _ = &mut overflow_receiver => {
                    let _ = send_stream(&mut session, StreamServerMessage::ResyncRequired).await;
                    let _ = session.close(None).await;
                    producer.abort();
                    return;
                },
                event = outbound_receiver.recv() => match event {
                    Some(event) => if send_stream(&mut session, StreamServerMessage::Event { event: event.into() }).await.is_err() { producer.abort(); return; },
                    None => { producer.abort(); return; }
                },
                message = messages.next() => match message {
                    Some(Ok(Message::Ping(bytes))) => {
                        if session.pong(&bytes).await.is_err() { producer.abort(); return; }
                    }
                    Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                        // L'accusé est intentionnellement indicatif en V1 : le
                        // curseur durable reste la responsabilité du client.
                        Ok(StreamClientMessage::Ack { .. } | StreamClientMessage::Ping) => {}
                        Ok(StreamClientMessage::Unsubscribe) => {
                            let _ = session.close(None).await;
                            producer.abort();
                            return;
                        }
                        Ok(StreamClientMessage::Subscribe { .. }) | Err(_) => {
                            let _ = session.close(None).await;
                            producer.abort();
                            return;
                        }
                    },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => { producer.abort(); return; },
                    _ => {}
                }
            }
        }
    });
    Ok(response)
}

/// `after = 0` est le curseur initial et signifie « commencer par la fenêtre
/// actuellement conservée ». Pour un curseur déjà émis, la première séquence
/// retenue doit être au plus `after + 1`, sinon il manque un événement purgé.
fn cursor_is_outside_retention(after: u64, earliest: Option<u64>) -> bool {
    after != 0 && earliest.is_some_and(|earliest| after < earliest.saturating_sub(1))
}

fn is_stream_topic(topic: &str) -> bool {
    matches!(
        topic,
        "state" | "device" | "area" | "automation" | "adapter" | "command"
    )
}

async fn send_stream(
    session: &mut actix_ws::Session,
    message: StreamServerMessage,
) -> Result<(), actix_ws::Closed> {
    session
        .text(serde_json::to_string(&message).expect("stream messages serialize"))
        .await
}

async fn authorize(
    request: &HttpRequest,
    state: &web::Data<ServerState>,
) -> Result<(), HttpResponse> {
    let Some(value) = bearer_from_header(request) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a local bearer token is required",
        ));
    };
    authorize_bearer(value, state).await
}

fn bearer_from_header(request: &HttpRequest) -> Option<String> {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned)
}

async fn authorize_stream(
    request: &HttpRequest,
    state: &web::Data<ServerState>,
) -> Result<(), HttpResponse> {
    let bearer = bearer_from_header(request).or_else(|| {
        request
            .cookie("robine_stream")
            .map(|cookie| cookie.value().to_owned())
    });
    let Some(bearer) = bearer else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a local bearer token is required",
        ));
    };
    authorize_bearer(bearer, state).await
}

async fn authorize_bearer(
    value: String,
    state: &web::Data<ServerState>,
) -> Result<(), HttpResponse> {
    let store = state.store.clone();
    match blocking(move || store.authenticate(&value)).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_invalid",
            "the bearer token is invalid",
        )),
        Err(response) => Err(response),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApplicationError> + Send + 'static,
) -> Result<T, HttpResponse> {
    match web::block(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(application_error(error)),
        Err(error) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        )),
    }
}

async fn blocking_flow<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, FlowError> + Send + 'static,
) -> Result<T, HttpResponse> {
    match web::block(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(flow_error(error)),
        Err(error) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_unavailable",
            &error.to_string(),
        )),
    }
}

async fn blocking_matter<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, MatterAdministrationError> + Send + 'static,
) -> Result<T, HttpResponse> {
    match web::block(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(matter_error(error)),
        Err(_) => Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "matter_unavailable",
            "Matter controller is unavailable",
        )),
    }
}

fn flow_error(error: FlowError) -> HttpResponse {
    match error {
        FlowError::NotFound => error_response(StatusCode::NOT_FOUND, "automation_not_found", "automation not found"),
        FlowError::Disabled => error_response(StatusCode::CONFLICT, "automation_disabled", "automation is disabled"),
        FlowError::AlreadyConsumed => error_response(
            StatusCode::CONFLICT,
            "automation_event_already_consumed",
            "automation already consumed the causal event",
        ),
        FlowError::Syntax(diagnostics) => HttpResponse::BadRequest().json(serde_json::json!({ "code": "flow_syntax_invalid", "diagnostics": diagnostics, "correlation_id": format!("cor_{}", uuid::Uuid::new_v4()) })),
        FlowError::Validation(diagnostics) => HttpResponse::BadRequest().json(serde_json::json!({ "code": "flow_validation_invalid", "diagnostics": diagnostics, "correlation_id": format!("cor_{}", uuid::Uuid::new_v4()) })),
        FlowError::Application(error) => application_error(error),
    }
}

fn application_error(error: ApplicationError) -> HttpResponse {
    match error {
        ApplicationError::EntityNotFound => error_response(
            StatusCode::NOT_FOUND,
            "entity_not_found",
            "entity not found",
        ),
        ApplicationError::CapabilityNotSupported(_)
        | ApplicationError::Validation(_)
        | ApplicationError::Domain(_) => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &error.to_string(),
        ),
        ApplicationError::Infrastructure(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "infrastructure_error",
            "the server could not complete the request",
        ),
    }
}
fn hue_error(error: HueAdministrationError) -> HttpResponse {
    match error {
        HueAdministrationError::ButtonNotPressed => error_response(
            StatusCode::CONFLICT,
            "hue_link_button_not_pressed",
            "press the physical Hue bridge button, then retry pairing",
        ),
        HueAdministrationError::InvalidRequest(_) => error_response(
            StatusCode::BAD_REQUEST,
            "hue_pairing_invalid",
            "Hue bridge identity could not be verified",
        ),
        HueAdministrationError::Unavailable(_) => error_response(
            StatusCode::BAD_GATEWAY,
            "hue_unavailable",
            "the Hue bridge could not be reached",
        ),
    }
}
fn matter_error(error: MatterAdministrationError) -> HttpResponse {
    match error {
        MatterAdministrationError::Unavailable => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "matter_unavailable",
            "Matter controller is unavailable",
        ),
        MatterAdministrationError::JobNotFound => error_response(
            StatusCode::NOT_FOUND,
            "matter_job_not_found",
            "Matter commissioning job was not found",
        ),
    }
}
fn error_response(status: StatusCode, code: &'static str, message: &str) -> HttpResponse {
    HttpResponse::build(status).json(ApiError {
        code,
        message: message.into(),
        correlation_id: format!("cor_{}", uuid::Uuid::new_v4()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;
    use robine_domain::{AdapterId, Capability, Command, DeviceDiscovery, DiscoveryEntity};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeHueAdministration(Mutex<Vec<String>>);
    impl HueAdministration for FakeHueAdministration {
        fn discover(&self) -> Result<Vec<HueBridgeCandidate>, HueAdministrationError> {
            Ok(Vec::new())
        }
        fn pair(&self, request: HuePairRequest) -> Result<HuePairResponse, HueAdministrationError> {
            self.0.lock().unwrap().push(request.authority);
            Ok(HuePairResponse {
                adapter_id: "hue:bridge-a".into(),
                discovered_devices: 2,
            })
        }
        fn synchronize(&self) -> Result<usize, HueAdministrationError> {
            Ok(2)
        }
        fn room_suggestions(&self) -> Result<Vec<HueRoomSuggestion>, HueAdministrationError> {
            Ok(vec![HueRoomSuggestion {
                name: "Salon Hue".into(),
                entity_ids: vec![EntityId(uuid::Uuid::nil())],
            }])
        }
        fn import_room(
            &self,
            suggestion: HueRoomSuggestion,
        ) -> Result<Area, HueAdministrationError> {
            (suggestion == self.room_suggestions()?[0])
                .then_some(Area {
                    id: AreaId::new(),
                    name: suggestion.name,
                })
                .ok_or_else(|| HueAdministrationError::InvalidRequest("unknown room".into()))
        }
    }
    struct FakeMatterAdministration;
    impl MatterAdministration for FakeMatterAdministration {
        fn start_commissioning(
            &self,
            setup_code: String,
        ) -> Result<String, MatterAdministrationError> {
            (setup_code == "34970112332")
                .then_some("job-1".into())
                .ok_or(MatterAdministrationError::Unavailable)
        }
        fn commissioning_job(
            &self,
            job_id: String,
        ) -> Result<CommissioningJob, MatterAdministrationError> {
            (job_id == "job-1")
                .then_some(CommissioningJob {
                    id: job_id,
                    status: robine_matter_contract::JobStatus::InProgress,
                    progress: 40,
                    detail: Some("Waiting for device".into()),
                })
                .ok_or(MatterAdministrationError::JobNotFound)
        }
    }
    struct FakeBackupAdministration;
    impl BackupAdministration for FakeBackupAdministration {
        fn create_backup(&self) -> Result<BackupResponse, ApplicationError> {
            Ok(BackupResponse {
                manifest_version: 1,
                created_at: "2026-08-07T12:00:00Z".into(),
                database_file: "robine-test.sqlite3".into(),
                bytes: 42,
                sha256: "a".repeat(64),
            })
        }
    }

    #[derive(Default)]
    struct RecordingCommandDispatcher(Mutex<Vec<Command>>);

    impl CommandDispatcher for RecordingCommandDispatcher {
        fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
            self.0.lock().unwrap().push(command);
            Ok(())
        }
    }
    #[actix_web::test]
    async fn protected_resources_reject_anonymous_calls() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/v1/devices").to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn administrator_can_issue_a_read_only_mcp_token_once() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store.clone())))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/auth/mcp-tokens")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({
                    "scopes": ["read"],
                    "expires_in_seconds": 86_400
                }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: serde_json::Value = test::read_body_json(response).await;
        let bearer = body["token"].as_str().expect("one-time bearer response");
        assert!(store.authenticate_mcp_read(bearer, Utc::now()).unwrap());
        assert_eq!(body["scopes"], serde_json::json!(["robine:read"]));
        assert!(body["token_id"].as_str().is_some());
    }

    #[actix_web::test]
    async fn browser_stream_session_is_http_only_scoped_and_short_lived() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/auth/stream-session")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|header| header.to_str().ok())
            .expect("stream session cookie");
        assert!(cookie.contains("robine_stream="));
        assert!(cookie.contains("Path=/api/v1/stream"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=600"));
    }

    #[actix_web::test]
    async fn stream_authentication_accepts_only_the_scoped_browser_cookie() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let state = web::Data::new(ServerState::new(service, store));
        let request = test::TestRequest::get()
            .uri("/api/v1/stream")
            .insert_header((header::COOKIE, format!("robine_stream={token}")))
            .to_http_request();
        assert!(authorize_stream(&request, &state).await.is_ok());

        let missing_cookie = test::TestRequest::get()
            .uri("/api/v1/stream")
            .to_http_request();
        assert!(authorize_stream(&missing_cookie, &state).await.is_err());
    }

    #[actix_web::test]
    async fn loopback_administrator_can_recover_a_lost_session_with_the_password() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store.clone())))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/auth/tokens")
                .peer_addr("127.0.0.1:3030".parse().unwrap())
                .set_json(serde_json::json!({ "password": "a suitably long password" }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(store.authenticate(body["token"].as_str().unwrap()).unwrap());

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/auth/tokens")
                .peer_addr("192.0.2.10:3030".parse().unwrap())
                .set_json(serde_json::json!({ "password": "a suitably long password" }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn health_reports_a_degraded_optional_adapter() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        service
            .update_adapter_health(robine_domain::AdapterHealth {
                adapter_id: AdapterId::new("mqtt:local").unwrap(),
                status: robine_domain::AdapterStatus::Degraded,
                detail: Some("broker unavailable".into()),
                observed_at: Utc::now(),
            })
            .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response =
            test::call_service(&app, test::TestRequest::get().uri("/health").to_request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let health: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(health["status"], "degraded");
        assert_eq!(
            health["degraded_adapters"],
            serde_json::json!(["mqtt:local"])
        );
    }

    #[actix_web::test]
    async fn authenticated_administrator_reads_non_secret_adapter_diagnostics() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        service
            .update_adapter_health(robine_domain::AdapterHealth {
                adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                status: robine_domain::AdapterStatus::Degraded,
                detail: Some("bridge unavailable".into()),
                observed_at: Utc::now(),
            })
            .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/adapters")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let adapters: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(adapters[0]["adapter_id"], "hue:bridge-a");
        assert_eq!(adapters[0]["detail"], "bridge unavailable");
    }

    #[actix_web::test]
    async fn event_tail_returns_only_the_latest_bounded_history() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        service.create_area("Salon".into(), Utc::now()).unwrap();
        service.create_area("Bureau".into(), Utc::now()).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/events?tail=1")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let page: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(page["events"].as_array().unwrap().len(), 1);
        assert_eq!(page["events"][0]["event_type"], "area.created");
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/events?tail=1&after=0")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn expired_event_cursor_returns_resync_required_instead_of_a_partial_replay() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let now = Utc::now();
        let expired_at = now - chrono::Duration::days(31);
        service.create_area("Ancien 1".into(), expired_at).unwrap();
        service.create_area("Ancien 2".into(), expired_at).unwrap();
        service.create_area("Actuel".into(), now).unwrap();
        assert_eq!(store.prune_retained_data(now, 10).unwrap().events, 2);
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/events?after=1")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let error: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(error["code"], "resync_required");
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_start_hue_pairing_without_receiving_a_secret() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let hue = Arc::new(FakeHueAdministration::default());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store).with_hue(hue.clone()),
                ))
                .configure(configure),
        )
        .await;
        let response = test::call_service(&app, test::TestRequest::post()
            .uri("/api/v1/adapters/hue/pair")
            .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
            .set_json(serde_json::json!({ "authority": "192.168.1.4", "certificate_pem": "-----BEGIN CERTIFICATE-----", "certificate_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }))
            .to_request()).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(*hue.0.lock().unwrap(), vec!["192.168.1.4"]);
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_preview_and_import_a_hue_room() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store)
                        .with_hue(Arc::new(FakeHueAdministration::default())),
                ))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/adapters/hue/rooms")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let suggestions: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(suggestions[0]["name"], "Salon Hue");
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/adapters/hue/rooms/import")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "suggestion": suggestions[0] }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            test::read_body_json::<serde_json::Value, _>(response).await["name"],
            "Salon Hue"
        );
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_synchronize_hue() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store)
                        .with_hue(Arc::new(FakeHueAdministration::default())),
                ))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/adapters/hue/synchronize")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            test::read_body_json::<serde_json::Value, _>(response).await["discovered_devices"],
            2
        );
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_start_and_read_a_matter_job() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store)
                        .with_matter(Arc::new(FakeMatterAdministration)),
                ))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/adapters/matter/commission")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "setup_code": "34970112332" }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            test::read_body_json::<serde_json::Value, _>(response).await["job_id"],
            "job-1"
        );
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/adapters/matter/jobs/job-1")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            test::read_body_json::<serde_json::Value, _>(response).await["job"]["progress"],
            40
        );
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_create_a_verified_backup_snapshot() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store)
                        .with_backups(Arc::new(FakeBackupAdministration)),
                ))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/backups")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            test::read_body_json::<serde_json::Value, _>(response).await["sha256"],
            "a".repeat(64)
        );
    }

    #[actix_web::test]
    async fn authenticated_administrator_assigns_a_light_to_an_area() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let device = service
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                    protocol_address: "light-a".into(),
                    name: "Lampe salon".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-a".into(),
                        name: "Lampe salon".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let area = service.create_area("Salon".into(), Utc::now()).unwrap();
        let entity_id = device.entities[0].id.clone();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::put()
                .uri(&format!("/api/v1/entities/{entity_id}/area"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "area_id": area.id.to_string() }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let entity: robine_domain::Entity = test::read_body_json(response).await;
        assert_eq!(entity.area_id, Some(area.id));
    }

    #[actix_web::test]
    async fn home_control_path_creates_a_room_assigns_a_hue_light_and_emits_command_events() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let dispatcher = Arc::new(RecordingCommandDispatcher::default());
        let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let hue = Arc::new(FakeHueAdministration::default());
        let device = service
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                    protocol_address: "light-a".into(),
                    name: "Lampe salon".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-a".into(),
                        name: "Lampe salon".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity_id = device.entities[0].id.clone();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(
                    ServerState::new(service, store).with_hue(hue.clone()),
                ))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/adapters/hue/pair")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({
                    "authority": "192.168.1.4",
                    "certificate_pem": "-----BEGIN CERTIFICATE-----",
                    "certificate_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(*hue.0.lock().unwrap(), vec!["192.168.1.4"]);

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/areas")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "name": "Salon" }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let area: robine_domain::Area = test::read_body_json(response).await;

        let response = test::call_service(
            &app,
            test::TestRequest::put()
                .uri(&format!("/api/v1/entities/{entity_id}/area"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "area_id": area.id.to_string() }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&format!("/api/v1/entities/{entity_id}/commands"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .insert_header(("Idempotency-Key", "living-room-light-on"))
                .set_json(serde_json::json!({ "key": "switch", "value": true }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/events?tail=10")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let events: serde_json::Value = test::read_body_json(response).await;
        let event_types = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| event["event_type"].as_str())
            .collect::<Vec<_>>();
        assert!(event_types.contains(&"area.created"));
        assert!(event_types.contains(&"command.dispatched"));
    }

    #[actix_web::test]
    async fn authenticated_administrator_reads_recent_persisted_automation_runs() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                "(flow (on (event :type \"test\")) (do (audit :message \"done\")))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        flows
            .execute_existing(&flow.id, &service, Utc::now())
            .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/api/v1/automations/{}/runs?limit=1", flow.id))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let runs: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(runs.as_array().unwrap().len(), 1);
        assert_eq!(runs[0]["flow_id"], flow.id.to_string());
        assert_eq!(runs[0]["result"]["status"], "completed");
    }

    #[actix_web::test]
    async fn authenticated_administrator_can_pause_an_automation_without_changing_its_source() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let source = r#"(flow (meta :name "Soir paisible") (on (event :type "test")) (do (audit :message "ok")))"#;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/automations")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "source": source, "enabled": true }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let created: serde_json::Value = test::read_body_json(response).await;
        let id = created["id"].as_str().unwrap();
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(&format!("/api/v1/automations/{id}/simulate"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let simulation: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(simulation["command_count"], 0);
        assert_eq!(simulation["result"]["status"], "completed");
        assert_eq!(simulation["result"]["steps"][0]["type"], "audit");
        let response = test::call_service(
            &app,
            test::TestRequest::patch()
                .uri(&format!("/api/v1/automations/{id}"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "source": source, "enabled": false }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let paused: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(paused["enabled"], false);
        assert_eq!(paused["source"], source);
        assert_eq!(paused["revision"], 2);
    }

    #[actix_web::test]
    async fn guided_hue_schedule_flow_is_validated_and_created_by_the_api() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        let device = service
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                    protocol_address: "light-a".into(),
                    name: "Lampe salon".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-a".into(),
                        name: "Lampe salon".into(),
                        kind: "light".into(),
                        capabilities: vec![
                            Capability::new("switch", 1).unwrap(),
                            Capability::new("light.brightness", 1).unwrap(),
                        ],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let source = format!(
            r#"(flow (meta :name "Lumière douce") (on (schedule :at "19:30" :timezone "Europe/Paris")) (do (command (entity "{entity}") :turn-on :brightness 40%)))"#
        );
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/v1/automations")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .set_json(serde_json::json!({ "source": source, "enabled": true }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let flow: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(flow["name"], "Lumière douce");
        assert_eq!(flow["source"], source);
    }

    #[actix_web::test]
    async fn device_list_is_bounded_and_uses_a_cursor_for_the_next_page() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let token = store
            .bootstrap_administrator("a suitably long password", Utc::now())
            .unwrap();
        let service = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(NoopCommandDispatcher),
        );
        for name in ["Aube", "Brume", "Cèdre"] {
            service
                .register_discovery(
                    DeviceDiscovery {
                        adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
                        protocol_address: format!("light-{name}"),
                        name: name.into(),
                        entities: vec![DiscoveryEntity {
                            protocol_address: format!("light-{name}"),
                            name: name.into(),
                            kind: "light".into(),
                            capabilities: vec![Capability::new("switch", 1).unwrap()],
                        }],
                    },
                    Utc::now(),
                )
                .unwrap();
        }
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ServerState::new(service, store)))
                .configure(configure),
        )
        .await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/v1/devices?limit=2&status=available")
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let first: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(first["devices"].as_array().unwrap().len(), 2);
        let cursor = first["next_cursor"].as_str().unwrap();
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/api/v1/devices?limit=2&cursor={cursor}"))
                .insert_header((header::AUTHORIZATION, format!("Bearer {token}")))
                .to_request(),
        )
        .await;
        let second: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(second["devices"].as_array().unwrap().len(), 1);
        assert!(second["next_cursor"].is_null());
    }
}
