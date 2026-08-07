//! Cas d'utilisation et ports de Robine.

use chrono::{DateTime, Utc};
use robine_domain::*;
use robine_flow_ast::FlowAst;
use robine_flow_check::{CapabilityCatalog, CheckDiagnostic, validate as validate_flow};
use robine_flow_syntax::parse as parse_flow;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use thiserror::Error;

pub trait HomeRepository: Send + Sync {
    fn register_discovery(
        &self,
        discovery: DeviceDiscovery,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError>;
    fn list_devices(&self) -> Result<Vec<Device>, ApplicationError>;
    fn get_entity(&self, id: &EntityId) -> Result<Option<Entity>, ApplicationError>;
    fn get_entity_state(&self, id: &EntityId) -> Result<Vec<StateProperty>, ApplicationError>;
    fn create_area(
        &self,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Area, EventEnvelope), ApplicationError>;
    fn list_areas(&self) -> Result<Vec<Area>, ApplicationError>;
    fn upsert_adapter_health(
        &self,
        health: AdapterHealth,
    ) -> Result<EventEnvelope, ApplicationError>;
    fn list_adapter_health(&self) -> Result<Vec<AdapterHealth>, ApplicationError>;
    fn create_flow(
        &self,
        flow: FlowDefinition,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError>;
    fn update_flow(
        &self,
        flow: FlowDefinition,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError>;
    fn get_flow(&self, id: &FlowId) -> Result<Option<FlowDefinition>, ApplicationError>;
    fn list_flows(&self) -> Result<Vec<FlowDefinition>, ApplicationError>;
    fn apply_reported_state(
        &self,
        state: ReportedState,
        now: DateTime<Utc>,
    ) -> Result<Vec<EventEnvelope>, ApplicationError>;
    fn create_command(
        &self,
        command: Command,
    ) -> Result<(Command, EventEnvelope), ApplicationError>;
    fn find_command_by_idempotency(&self, key: &str) -> Result<Option<Command>, ApplicationError>;
    fn transition_command(
        &self,
        id: &CommandId,
        status: CommandStatus,
        reason: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError>;
    fn events_after(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError>;
}

pub trait EventStream: Send + Sync {
    fn publish(&self, event: EventEnvelope);
}

pub trait CommandDispatcher: Send + Sync {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError>;
}

#[derive(Clone)]
pub struct HomeService {
    repository: Arc<dyn HomeRepository>,
    events: Arc<dyn EventStream>,
    dispatcher: Arc<dyn CommandDispatcher>,
}

#[derive(Clone)]
pub struct FlowService {
    repository: Arc<dyn HomeRepository>,
    events: Arc<dyn EventStream>,
}

impl FlowService {
    pub fn new(repository: Arc<dyn HomeRepository>, events: Arc<dyn EventStream>) -> Self {
        Self { repository, events }
    }

    pub fn create(
        &self,
        source: String,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<FlowDefinition, FlowError> {
        let ast = parse_and_validate(&source, self.repository.as_ref())?;
        let flow = FlowDefinition {
            id: FlowId::new(),
            name: flow_name(&ast).unwrap_or_else(|| "Nouvelle habitude".into()),
            enabled,
            revision: 1,
            ast: serde_json::to_value(&ast).expect("Flow AST serializes"),
            source_hash: source_hash(&source),
            source,
        };
        let event = self
            .repository
            .create_flow(flow.clone(), now)
            .map_err(FlowError::Application)?;
        self.events.publish(event);
        Ok(flow)
    }

    pub fn update(
        &self,
        id: FlowId,
        source: String,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<FlowDefinition, FlowError> {
        let previous = self
            .repository
            .get_flow(&id)
            .map_err(FlowError::Application)?
            .ok_or(FlowError::NotFound)?;
        let ast = parse_and_validate(&source, self.repository.as_ref())?;
        let flow = FlowDefinition {
            id,
            name: flow_name(&ast).unwrap_or(previous.name),
            enabled,
            revision: previous.revision + 1,
            ast: serde_json::to_value(&ast).expect("Flow AST serializes"),
            source_hash: source_hash(&source),
            source,
        };
        let event = self
            .repository
            .update_flow(flow.clone(), now)
            .map_err(FlowError::Application)?;
        self.events.publish(event);
        Ok(flow)
    }

    pub fn list(&self) -> Result<Vec<FlowDefinition>, FlowError> {
        self.repository.list_flows().map_err(FlowError::Application)
    }

    pub fn get(&self, id: &FlowId) -> Result<FlowDefinition, FlowError> {
        self.repository
            .get_flow(id)
            .map_err(FlowError::Application)?
            .ok_or(FlowError::NotFound)
    }

    pub fn simulate(&self, source: String) -> Result<FlowSimulation, FlowError> {
        let ast = parse_and_validate(&source, self.repository.as_ref())?;
        Ok(FlowSimulation {
            name: flow_name(&ast),
            command_count: count_forms(&ast.root, "command"),
            diagnostics: Vec::new(),
        })
    }

    pub fn simulate_existing(&self, id: &FlowId) -> Result<FlowSimulation, FlowError> {
        self.simulate(self.get(id)?.source)
    }
}

struct RepositoryCatalog<'a> {
    repository: &'a dyn HomeRepository,
}
impl CapabilityCatalog for RepositoryCatalog<'_> {
    fn capabilities_for(&self, entity_id: &str) -> Option<std::collections::HashSet<String>> {
        let id = uuid::Uuid::parse_str(entity_id).ok()?;
        self.repository
            .get_entity(&EntityId(id))
            .ok()
            .flatten()
            .map(|entity| {
                entity
                    .capabilities
                    .into_iter()
                    .map(|capability| capability.key)
                    .collect()
            })
    }
}

fn parse_and_validate(source: &str, repository: &dyn HomeRepository) -> Result<FlowAst, FlowError> {
    let ast = parse_flow(source).map_err(FlowError::Syntax)?;
    let diagnostics = validate_flow(&ast, &RepositoryCatalog { repository });
    if diagnostics.is_empty() {
        Ok(ast)
    } else {
        Err(FlowError::Validation(diagnostics))
    }
}
fn flow_name(ast: &FlowAst) -> Option<String> {
    let forms = ast.root.list()?;
    let meta = forms
        .iter()
        .find(|form| {
            form.list()
                .and_then(|items| items.first())
                .and_then(|form| form.symbol())
                == Some("meta")
        })?
        .list()?;
    meta.windows(2).find_map(|pair| {
        matches!(&pair[0], robine_flow_ast::Form::Keyword(key) if key == "name")
            .then(|| match &pair[1] {
                robine_flow_ast::Form::String(value) => Some(value.clone()),
                _ => None,
            })
            .flatten()
    })
}
fn source_hash(source: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(source.as_bytes()))
}
fn count_forms(form: &robine_flow_ast::Form, symbol: &str) -> usize {
    match form {
        robine_flow_ast::Form::List(items) => {
            usize::from(items.first().and_then(|form| form.symbol()) == Some(symbol))
                + items
                    .iter()
                    .map(|item| count_forms(item, symbol))
                    .sum::<usize>()
        }
        _ => 0,
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowSimulation {
    pub name: Option<String>,
    pub command_count: usize,
    pub diagnostics: Vec<CheckDiagnostic>,
}

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("Flow syntax is invalid")]
    Syntax(Vec<robine_flow_syntax::Diagnostic>),
    #[error("Flow validation failed")]
    Validation(Vec<CheckDiagnostic>),
    #[error("automation not found")]
    NotFound,
    #[error(transparent)]
    Application(#[from] ApplicationError),
}

impl HomeService {
    pub fn new(
        repository: Arc<dyn HomeRepository>,
        events: Arc<dyn EventStream>,
        dispatcher: Arc<dyn CommandDispatcher>,
    ) -> Self {
        Self {
            repository,
            events,
            dispatcher,
        }
    }
    pub fn register_discovery(
        &self,
        discovery: DeviceDiscovery,
        now: DateTime<Utc>,
    ) -> Result<Device, ApplicationError> {
        discovery.validate().map_err(ApplicationError::Domain)?;
        let (device, event) = self.repository.register_discovery(discovery, now)?;
        self.events.publish(event);
        Ok(device)
    }
    pub fn list_devices(&self) -> Result<Vec<Device>, ApplicationError> {
        self.repository.list_devices()
    }
    pub fn entity_detail(&self, id: &EntityId) -> Result<Option<EntityDetail>, ApplicationError> {
        let Some(entity) = self.repository.get_entity(id)? else {
            return Ok(None);
        };
        Ok(Some(EntityDetail {
            state: self.repository.get_entity_state(id)?,
            entity,
        }))
    }
    pub fn create_area(&self, name: String, now: DateTime<Utc>) -> Result<Area, ApplicationError> {
        if name.trim().is_empty() {
            return Err(ApplicationError::Validation(
                "area name must not be empty".into(),
            ));
        }
        let (area, event) = self.repository.create_area(name, now)?;
        self.events.publish(event);
        Ok(area)
    }
    pub fn list_areas(&self) -> Result<Vec<Area>, ApplicationError> {
        self.repository.list_areas()
    }
    pub fn update_adapter_health(&self, health: AdapterHealth) -> Result<(), ApplicationError> {
        let event = self.repository.upsert_adapter_health(health)?;
        self.events.publish(event);
        Ok(())
    }
    pub fn list_adapter_health(&self) -> Result<Vec<AdapterHealth>, ApplicationError> {
        self.repository.list_adapter_health()
    }
    pub fn apply_reported_state(
        &self,
        state: ReportedState,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        for event in self.repository.apply_reported_state(state, now)? {
            self.events.publish(event);
        }
        Ok(())
    }
    pub fn request_command(
        &self,
        entity_id: EntityId,
        key: String,
        value: StateValue,
        idempotency_key: String,
        now: DateTime<Utc>,
    ) -> Result<Command, ApplicationError> {
        if idempotency_key.trim().is_empty() {
            return Err(ApplicationError::Validation(
                "Idempotency-Key is required".into(),
            ));
        }
        if let Some(command) = self
            .repository
            .find_command_by_idempotency(&idempotency_key)?
        {
            return Ok(command);
        }
        let entity = self
            .repository
            .get_entity(&entity_id)?
            .ok_or(ApplicationError::EntityNotFound)?;
        if !entity.supports(&key) {
            return Err(ApplicationError::CapabilityNotSupported(key));
        }
        if !value.is_valid_for(&key) {
            return Err(ApplicationError::Domain(DomainError::InvalidStateValue(
                key,
            )));
        }
        let command = Command {
            id: CommandId::new(),
            entity_id,
            key,
            value,
            correlation_id: format!("cor_{}", uuid::Uuid::new_v4()),
            idempotency_key,
            requested_at: now,
            status: CommandStatus::Requested,
        };
        let (mut command, event) = self.repository.create_command(command)?;
        self.events.publish(event);
        match self.dispatcher.dispatch(command.clone()) {
            Ok(()) => {
                let event = self.repository.transition_command(
                    &command.id,
                    CommandStatus::Dispatched,
                    None,
                    now,
                )?;
                command.status = CommandStatus::Dispatched;
                self.events.publish(event);
                Ok(command)
            }
            Err(error) => {
                let event = self.repository.transition_command(
                    &command.id,
                    CommandStatus::Failed,
                    Some(error.to_string()),
                    now,
                )?;
                self.events.publish(event);
                Err(error)
            }
        }
    }
    pub fn events_after(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError> {
        self.repository.events_after(after, limit)
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EntityDetail {
    pub entity: Entity,
    pub state: Vec<StateProperty>,
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("entity not found")]
    EntityNotFound,
    #[error("capability not supported: {0}")]
    CapabilityNotSupported(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("infrastructure failure: {0}")]
    Infrastructure(String),
}
