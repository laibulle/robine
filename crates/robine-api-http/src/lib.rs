//! Adaptateur HTTP Actix : il traduit le contrat réseau vers les cas d'utilisation.

use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer,
    http::{StatusCode, header},
    web,
};
use actix_ws::Message;
use chrono::Utc;
use futures_util::StreamExt;
use robine_api_contract::*;
use robine_application::{
    ApplicationError, CommandDispatcher, FlowError, FlowService, HomeService,
};
use robine_domain::{Command, EntityId, FlowId};
use robine_store_sqlite::SqliteStore;
use std::sync::Arc;

#[derive(Clone)]
pub struct ServerState {
    pub service: HomeService,
    pub flows: FlowService,
    pub store: Arc<SqliteStore>,
}
impl ServerState {
    pub fn new(service: HomeService, store: Arc<SqliteStore>) -> Self {
        Self {
            flows: FlowService::new(store.clone(), store.clone()),
            service,
            store,
        }
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
                .route("/devices", web::get().to(list_devices))
                .route("/entities/{id}", web::get().to(entity_detail))
                .route("/entities/{id}/commands", web::post().to(request_command))
                .route("/areas", web::get().to(list_areas))
                .route("/areas", web::post().to(create_area))
                .route("/adapters", web::get().to(list_adapters))
                .route("/automations", web::get().to(list_automations))
                .route("/automations", web::post().to(create_automation))
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
    match blocking(move || store.is_initialized()).await {
        Ok(initialized) => HttpResponse::Ok()
            .json(serde_json::json!({ "status": "ok", "initialized": initialized })),
        Err(response) => response,
    }
}

async fn bootstrap_administrator(
    request: HttpRequest,
    state: web::Data<ServerState>,
    body: web::Json<BootstrapAdministratorRequest>,
) -> HttpResponse {
    if !request
        .peer_addr()
        .is_some_and(|address| address.ip().is_loopback())
    {
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
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let password = body.into_inner().password;
    let store = state.store.clone();
    match blocking(move || store.issue_token(&password, Utc::now())).await {
        Ok(token) => HttpResponse::build(StatusCode::CREATED).json(TokenResponse { token }),
        Err(response) => response,
    }
}

async fn list_devices(request: HttpRequest, state: web::Data<ServerState>) -> HttpResponse {
    if let Err(response) = authorize(&request, &state).await {
        return response;
    }
    let service = state.service.clone();
    match blocking(move || service.list_devices()).await {
        Ok(devices) => HttpResponse::Ok().json(devices),
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
    let after = query
        .get("after")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let service = state.service.clone();
    match blocking(move || service.events_after(after, 500)).await {
        Ok(events) => HttpResponse::Ok().json(events),
        Err(response) => response,
    }
}

async fn stream(
    request: HttpRequest,
    body: web::Payload,
    state: web::Data<ServerState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Err(response) = authorize(&request, &state).await {
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
        let mut receiver = store.subscribe_events();
        let after = after.unwrap_or(0);
        let replay = service.events_after(after, 500).unwrap_or_default();
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
            if send_stream(&mut session, StreamServerMessage::Event { event })
                .await
                .is_err()
            {
                return;
            }
        }
        loop {
            tokio::select! {
                event = receiver.recv() => match event {
                    Ok(event) if event.sequence > after && topics.iter().any(|topic| topic == event.data.topic()) => if send_stream(&mut session, StreamServerMessage::Event { event }).await.is_err() { return; },
                    Ok(_) => {}, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { let _ = send_stream(&mut session, StreamServerMessage::ResyncRequired).await; let _ = session.close(None).await; return; }, Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                message = messages.next() => match message { Some(Ok(Message::Ping(bytes))) => { if session.pong(&bytes).await.is_err() { return; } }, Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return, _ => {} }
            }
        }
    });
    Ok(response)
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
    let Some(value) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned)
    else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_required",
            "a local bearer token is required",
        ));
    };
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

fn flow_error(error: FlowError) -> HttpResponse {
    match error {
        FlowError::NotFound => error_response(StatusCode::NOT_FOUND, "automation_not_found", "automation not found"),
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
    use std::sync::Arc;
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
}
