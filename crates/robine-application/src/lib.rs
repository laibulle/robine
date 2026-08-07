//! Cas d'utilisation et ports de Robine.

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use robine_domain::*;
use robine_flow_ast::{FlowAst, Form};
use robine_flow_check::{CapabilityCatalog, CheckDiagnostic, validate as validate_flow};
use robine_flow_plan::{ConcurrencyPolicy, compile as compile_flow};
use robine_flow_runtime::{
    CommandGateway, RunId, RunResult, RunTrace, TraceStep, execute as execute_plan,
    execute_from_resumption,
};
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
    fn list_devices_page(&self, request: DevicePageRequest)
    -> Result<DevicePage, ApplicationError>;
    fn rename_device(
        &self,
        id: &DeviceId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError>;
    fn remove_device(
        &self,
        id: &DeviceId,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError>;
    fn get_entity(&self, id: &EntityId) -> Result<Option<Entity>, ApplicationError>;
    fn rename_entity(
        &self,
        id: &EntityId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Entity, EventEnvelope), ApplicationError>;
    fn is_entity_commandable(&self, id: &EntityId) -> Result<bool, ApplicationError>;
    fn get_entity_state(&self, id: &EntityId) -> Result<Vec<StateProperty>, ApplicationError>;
    fn create_area(
        &self,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Area, EventEnvelope), ApplicationError>;
    fn list_areas(&self) -> Result<Vec<Area>, ApplicationError>;
    fn assign_entity_area(
        &self,
        entity_id: &EntityId,
        area_id: Option<&AreaId>,
        now: DateTime<Utc>,
    ) -> Result<(Entity, EventEnvelope), ApplicationError>;
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
    fn save_flow_run(&self, run: FlowRun) -> Result<(), ApplicationError>;
    /// Admission atomique d'une exécution selon la politique compilée. SQLite
    /// décide sous transaction afin que deux événements concurrents ne puissent
    /// pas contourner `single`, `restart` ou la capacité de `queue`.
    fn admit_flow_run(
        &self,
        run: FlowRun,
        policy: &ConcurrencyPolicy,
    ) -> Result<FlowAdmission, ApplicationError>;
    /// Retire atomiquement la première exécution en attente d'une file dont le
    /// Flow n'a plus d'exécution active.
    fn dequeue_next_flow_run(&self) -> Result<Option<FlowRun>, ApplicationError>;
    fn due_flow_runs(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<FlowRun>, ApplicationError>;
    fn awaiting_flow_runs(&self, limit: usize) -> Result<Vec<FlowRun>, ApplicationError>;
    fn delete_flow_run(&self, id: &FlowRunId) -> Result<(), ApplicationError>;
    /// Trace sérialisée d'une exécution, mise à jour à chaque suspension ou
    /// transition terminale. Elle ne contient jamais de secrets d'adaptateur.
    fn save_flow_trace(
        &self,
        id: &FlowRunId,
        flow_id: &FlowId,
        result: serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError>;
    fn get_flow_trace(&self, id: &FlowRunId)
    -> Result<Option<serde_json::Value>, ApplicationError>;
    fn list_flow_traces(
        &self,
        flow_id: &FlowId,
        limit: usize,
    ) -> Result<Vec<FlowRunTrace>, ApplicationError>;
    /// Réserve atomiquement l'exécution d'un Flow pour une chaîne causale.
    /// `false` signifie que ce Flow a déjà consommé l'événement.
    fn claim_flow_trigger(
        &self,
        flow_id: &FlowId,
        correlation_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, ApplicationError>;
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
    fn expire_commands(
        &self,
        before: DateTime<Utc>,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError>;
    fn events_after(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError>;
    fn recent_events(&self, limit: usize) -> Result<Vec<EventEnvelope>, ApplicationError>;
    fn latest_event_sequence(&self) -> Result<u64, ApplicationError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum FlowAdmission {
    Start,
    Queued,
    Rejected { active_runs: usize },
    Restarted { cancelled: Vec<FlowRun> },
}

/// Paramètres bornés d'une liste d'appareils. Le curseur est l'identifiant du
/// dernier élément effectivement retourné, jamais un offset mutable.
#[derive(Clone, Debug)]
pub struct DevicePageRequest {
    pub cursor: Option<DeviceId>,
    pub limit: usize,
    pub status: Option<DeviceStatus>,
}

#[derive(Clone, Debug)]
pub struct DevicePage {
    pub devices: Vec<Device>,
    pub next_cursor: Option<DeviceId>,
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

#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowExecution {
    pub flow_id: FlowId,
    pub run_id: RunId,
    pub result: RunResult,
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

    fn set_enabled_from_flow(
        &self,
        flow_id: &str,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(flow_id)
            .map(FlowId)
            .map_err(|_| "Flow reference is not a UUID".to_string())?;
        let mut flow = self
            .repository
            .get_flow(&id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "referenced automation does not exist".to_string())?;
        if flow.enabled == enabled {
            return Ok(());
        }
        flow.enabled = enabled;
        flow.revision += 1;
        let event = self
            .repository
            .update_flow(flow, now)
            .map_err(|error| error.to_string())?;
        self.events.publish(event);
        Ok(())
    }

    pub fn explain_run(&self, run_id: &str) -> Result<serde_json::Value, FlowError> {
        let id = FlowRunId(uuid::Uuid::parse_str(run_id).map_err(|_| FlowError::NotFound)?);
        self.repository
            .get_flow_trace(&id)
            .map_err(FlowError::Application)?
            .ok_or(FlowError::NotFound)
    }

    pub fn list_runs(&self, id: &FlowId, limit: usize) -> Result<Vec<FlowRunTrace>, FlowError> {
        if !(1..=100).contains(&limit) {
            return Err(FlowError::Application(ApplicationError::Validation(
                "run history limit must be between 1 and 100".into(),
            )));
        }
        self.get(id)?;
        self.repository
            .list_flow_traces(id, limit)
            .map_err(FlowError::Application)
    }

    pub fn simulate(&self, source: String) -> Result<FlowSimulation, FlowError> {
        let ast = parse_and_validate(&source, self.repository.as_ref())?;
        let resolved_ast = resolve_choose_actions(&ast, self.repository.as_ref())?;
        let plan = compile_flow(&resolved_ast).map_err(|error| {
            FlowError::Application(ApplicationError::Validation(error.to_string()))
        })?;
        let result = if evaluate_guard(&ast, self.repository.as_ref())? {
            execute_plan(&plan, RunId::new(), &SimulationGateway).map_err(|error| {
                FlowError::Application(ApplicationError::Infrastructure(error.to_string()))
            })?
        } else {
            RunResult::Skipped(RunTrace {
                steps: vec![TraceStep::GuardEvaluated {
                    passed: false,
                    summary: "La condition de l'habitude n'est pas remplie.".into(),
                }],
            })
        };
        Ok(FlowSimulation {
            name: flow_name(&ast),
            command_count: planned_command_count(&plan.actions),
            diagnostics: Vec::new(),
            result,
        })
    }

    pub fn simulate_existing(&self, id: &FlowId) -> Result<FlowSimulation, FlowError> {
        self.simulate(self.get(id)?.source)
    }

    /// Exécute le plan immuable d'un Flow activé. Les commandes passent par le
    /// même cas d'utilisation que les commandes API afin de conserver la
    /// validation, la corrélation et l'idempotence.
    pub fn execute_existing(
        &self,
        id: &FlowId,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Result<FlowExecution, FlowError> {
        let flow = self.get(id)?;
        if !flow.enabled {
            return Err(FlowError::Disabled);
        }
        self.execute_flow(
            flow,
            commands,
            now,
            format!("flow:manual:{}", uuid::Uuid::new_v4()),
        )
    }

    fn execute_flow(
        &self,
        flow: FlowDefinition,
        commands: &HomeService,
        now: DateTime<Utc>,
        correlation_id: String,
    ) -> Result<FlowExecution, FlowError> {
        let ast: FlowAst = serde_json::from_value(flow.ast).map_err(|error| {
            FlowError::Application(ApplicationError::Infrastructure(error.to_string()))
        })?;
        let resolved_ast = resolve_choose_actions(&ast, self.repository.as_ref())?;
        let plan = compile_flow(&resolved_ast).map_err(|error| {
            FlowError::Application(ApplicationError::Validation(error.to_string()))
        })?;
        let run_id = RunId::new();
        if !evaluate_guard(&ast, self.repository.as_ref())? {
            let result = RunResult::Skipped(RunTrace {
                steps: vec![TraceStep::GuardEvaluated {
                    passed: false,
                    summary: "La condition de l'habitude n'est pas remplie.".into(),
                }],
            });
            self.persist_execution_trace(&flow.id, &run_id, &result, now)?;
            return Ok(FlowExecution {
                flow_id: flow.id,
                run_id,
                result,
            });
        }
        let run = FlowRun {
            id: FlowRunId(run_id.0),
            flow_id: flow.id.clone(),
            plan: serde_json::to_value(&plan).expect("execution plan serializes"),
            trace: None,
            retry_attempt: None,
            next_action: 0,
            wake_at: now,
            awaiting: None,
            correlation_id,
            deadline_at: plan
                .max_runtime_milliseconds
                .map(|milliseconds| now + chrono::Duration::milliseconds(milliseconds as i64)),
            queued: false,
        };
        let admission = self
            .repository
            .admit_flow_run(run.clone(), &plan.concurrency)
            .map_err(FlowError::Application)?;
        match admission {
            FlowAdmission::Start => self.resume_run(run, commands, now),
            FlowAdmission::Restarted { cancelled } => {
                for previous in cancelled {
                    let cancelled = RunResult::Cancelled(RunTrace {
                        steps: vec![TraceStep::Audit {
                            message: "cancelled by a newer :restart execution".into(),
                        }],
                    });
                    self.persist_execution_trace(
                        &previous.flow_id,
                        &RunId(previous.id.0),
                        &cancelled,
                        now,
                    )?;
                }
                self.resume_run(run, commands, now)
            }
            FlowAdmission::Queued => {
                let result = RunResult::Queued(RunTrace {
                    steps: vec![TraceStep::Audit {
                        message: "queued behind an active execution".into(),
                    }],
                });
                self.persist_execution_trace(&flow.id, &run_id, &result, now)?;
                Ok(FlowExecution {
                    flow_id: flow.id,
                    run_id,
                    result,
                })
            }
            FlowAdmission::Rejected { active_runs } => {
                let result = RunResult::Skipped(RunTrace {
                    steps: vec![TraceStep::Audit {
                        message: format!(
                            "execution skipped: concurrency limit reached ({active_runs})"
                        ),
                    }],
                });
                self.persist_execution_trace(&flow.id, &run_id, &result, now)?;
                Ok(FlowExecution {
                    flow_id: flow.id,
                    run_id,
                    result,
                })
            }
        }
    }

    /// Retourne les Flows activés dont le déclencheur V1 correspond à un état
    /// rapporté. Le runtime appelle cette méthode uniquement après persistance
    /// de l'événement dans le journal.
    pub fn execute_state_triggered(
        &self,
        state: &StateProperty,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        let mut executions = self.resume_awaiting_state(state, commands, now);
        executions.extend(
            self.list()
                .unwrap_or_default()
                .into_iter()
                .filter(|flow| flow.enabled)
                .filter_map(|flow| {
                    let ast: FlowAst = serde_json::from_value(flow.ast.clone()).ok()?;
                    state_trigger_matches(&ast, state)
                        .then(|| self.execute_existing(&flow.id, commands, now))
                })
                .collect::<Vec<_>>(),
        );
        executions
    }

    /// Exécute les Flows activés dont le déclencheur événementiel correspond à
    /// l'enveloppe déjà persistée. Les adaptateurs n'invoquent jamais ce code
    /// directement : le runtime consomme uniquement le journal confirmé.
    pub fn execute_event_triggered(
        &self,
        event: &EventEnvelope,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        let correlation_id = event
            .correlation_id
            .clone()
            .unwrap_or_else(|| format!("event:{}", event.sequence));
        let mut executions = self.resume_awaiting_event(event, commands, now);
        executions.extend(
            self.list()
                .unwrap_or_default()
                .into_iter()
                .filter(|flow| flow.enabled)
                .filter_map(|flow| {
                    let ast: FlowAst = serde_json::from_value(flow.ast.clone()).ok()?;
                    event_trigger_matches(&ast, event).then_some(flow)
                })
                .map(|flow| {
                    match self
                        .repository
                        .claim_flow_trigger(&flow.id, &correlation_id, now)
                        .map_err(FlowError::Application)?
                    {
                        true => self.execute_flow(flow, commands, now, correlation_id.clone()),
                        false => Err(FlowError::AlreadyConsumed),
                    }
                })
                .filter(|result| !matches!(result, Err(FlowError::AlreadyConsumed)))
                .collect::<Vec<_>>(),
        );
        executions
    }

    /// Exécute les Flows dont l'horaire civil local correspond à `now`. La
    /// réservation causale inclut la minute locale : le tick du runtime peut
    /// passer plusieurs fois dans la minute sans déclencher deux commandes,
    /// et le repli d'heure d'été reste une occurrence unique.
    pub fn execute_scheduled(
        &self,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        self.list()
            .unwrap_or_default()
            .into_iter()
            .filter(|flow| flow.enabled)
            .filter_map(|flow| {
                let ast: FlowAst = serde_json::from_value(flow.ast.clone()).ok()?;
                schedule_trigger_key(&ast, now).map(|key| (flow, key))
            })
            .map(|(flow, correlation_id)| {
                match self
                    .repository
                    .claim_flow_trigger(&flow.id, &correlation_id, now)
                    .map_err(FlowError::Application)?
                {
                    true => self.execute_flow(flow, commands, now, correlation_id),
                    false => Err(FlowError::AlreadyConsumed),
                }
            })
            .filter(|result| !matches!(result, Err(FlowError::AlreadyConsumed)))
            .collect()
    }

    /// Reprend les délais déjà persistés. Chaque ligne est supprimée seulement
    /// lorsque le plan atteint un état terminal ; l'opération est donc sûre à
    /// relancer après le redémarrage du processus.
    pub fn resume_due(
        &self,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        let mut executions = self
            .repository
            .due_flow_runs(now, 100)
            .unwrap_or_default()
            .into_iter()
            .map(|run| self.resume_run(run, commands, now))
            .collect::<Vec<_>>();
        for _ in 0..100 {
            let next = match self.repository.dequeue_next_flow_run() {
                Ok(next) => next,
                Err(error) => {
                    executions.push(Err(FlowError::Application(error)));
                    break;
                }
            };
            let Some(run) = next else { break };
            executions.push(self.resume_run(run, commands, now));
        }
        executions
    }

    fn resume_awaiting_event(
        &self,
        event: &EventEnvelope,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        self.repository
            .awaiting_flow_runs(100)
            .unwrap_or_default()
            .into_iter()
            .filter(|run| await_matches_event(run, event))
            .map(|run| self.resume_run(run, commands, now))
            .collect()
    }

    fn resume_awaiting_state(
        &self,
        state: &StateProperty,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Vec<Result<FlowExecution, FlowError>> {
        self.repository
            .awaiting_flow_runs(100)
            .unwrap_or_default()
            .into_iter()
            .filter(|run| await_matches_state(run, state))
            .map(|run| self.resume_run(run, commands, now))
            .collect()
    }

    fn resume_run(
        &self,
        run: FlowRun,
        commands: &HomeService,
        now: DateTime<Utc>,
    ) -> Result<FlowExecution, FlowError> {
        if run.deadline_at.is_some_and(|deadline| deadline <= now) {
            let run_id = RunId(run.id.0);
            let mut trace = trace_for_run(&run)?;
            trace.steps.push(TraceStep::Audit {
                message: "max-runtime reached before the flow could resume".into(),
            });
            let result = RunResult::TimedOut(trace);
            self.repository
                .delete_flow_run(&run.id)
                .map_err(FlowError::Application)?;
            self.persist_execution_trace(&run.flow_id, &run_id, &result, now)?;
            return Ok(FlowExecution {
                flow_id: run.flow_id,
                run_id,
                result,
            });
        }
        let flow = self.get(&run.flow_id)?;
        if !flow.enabled {
            let run_id = RunId(run.id.0);
            let mut trace = trace_for_run(&run)?;
            trace.steps.push(TraceStep::Audit {
                message: "cancelled because the automation was disabled".into(),
            });
            let result = RunResult::Cancelled(trace);
            self.repository
                .delete_flow_run(&run.id)
                .map_err(FlowError::Application)?;
            self.persist_execution_trace(&run.flow_id, &run_id, &result, now)?;
            return Ok(FlowExecution {
                flow_id: run.flow_id,
                run_id,
                result,
            });
        }
        let plan = serde_json::from_value(run.plan.clone()).map_err(|error| {
            FlowError::Application(ApplicationError::Infrastructure(error.to_string()))
        })?;
        let run_id = RunId(run.id.0);
        let result = execute_from_resumption(
            &plan,
            run_id.clone(),
            &HomeCommandGateway {
                commands,
                flows: self,
                now,
                correlation_id: if run.correlation_id.is_empty() {
                    format!("flow:resumed:{}", run.id)
                } else {
                    run.correlation_id.clone()
                },
            },
            run.next_action,
            run.retry_attempt,
            trace_for_run(&run)?,
        )
        .map_err(|error| {
            FlowError::Application(ApplicationError::Infrastructure(error.to_string()))
        })?;
        match &result {
            RunResult::Suspended {
                after_milliseconds,
                next_action,
                awaiting,
                retry_attempt,
                ..
            } => self
                .repository
                .save_flow_run(FlowRun {
                    id: run.id.clone(),
                    flow_id: run.flow_id.clone(),
                    plan: serde_json::to_value(&plan).expect("execution plan serializes"),
                    trace: Some(
                        serde_json::to_value(result.trace()).expect("run trace serializes"),
                    ),
                    retry_attempt: *retry_attempt,
                    next_action: *next_action,
                    wake_at: suspension_wake_at(now, *after_milliseconds, run.deadline_at),
                    awaiting: awaiting.as_ref().map(|trigger| {
                        serde_json::to_value(trigger).expect("await trigger serializes")
                    }),
                    correlation_id: run.correlation_id,
                    deadline_at: run.deadline_at,
                    queued: false,
                })
                .map_err(FlowError::Application)?,
            RunResult::Completed(_)
            | RunResult::Failed(_)
            | RunResult::Skipped(_)
            | RunResult::TimedOut(_)
            | RunResult::Cancelled(_)
            | RunResult::Queued(_) => self
                .repository
                .delete_flow_run(&run.id)
                .map_err(FlowError::Application)?,
        }
        self.persist_execution_trace(&flow.id, &run_id, &result, now)?;
        Ok(FlowExecution {
            flow_id: flow.id,
            run_id,
            result,
        })
    }

    fn persist_execution_trace(
        &self,
        flow_id: &FlowId,
        run_id: &RunId,
        result: &RunResult,
        now: DateTime<Utc>,
    ) -> Result<(), FlowError> {
        self.repository
            .save_flow_trace(
                &FlowRunId(run_id.0),
                flow_id,
                serde_json::to_value(result).expect("Flow runtime result serializes"),
                now,
            )
            .map_err(FlowError::Application)
    }
}

fn planned_command_count(actions: &[robine_flow_plan::PlannedAction]) -> usize {
    actions
        .iter()
        .map(|action| match action {
            robine_flow_plan::PlannedAction::Command { .. } => 1,
            robine_flow_plan::PlannedAction::Retry { action, .. } => {
                planned_command_count(std::slice::from_ref(action.as_ref()))
            }
            _ => 0,
        })
        .sum()
}

fn trace_for_run(run: &FlowRun) -> Result<RunTrace, FlowError> {
    run.trace
        .as_ref()
        .map(|trace| {
            serde_json::from_value(trace.clone()).map_err(|error| {
                FlowError::Application(ApplicationError::Infrastructure(format!(
                    "persisted Flow trace is invalid: {error}"
                )))
            })
        })
        .transpose()
        .map(|trace| trace.unwrap_or(RunTrace { steps: Vec::new() }))
}

fn suspension_wake_at(
    now: DateTime<Utc>,
    after_milliseconds: Option<u64>,
    deadline_at: Option<DateTime<Utc>>,
) -> DateTime<Utc> {
    let wake_at = after_milliseconds
        .map(|milliseconds| now + chrono::Duration::milliseconds(milliseconds as i64))
        // Une attente sans timeout n'est jamais sélectionnée par `due_flow_runs`
        // tant que la colonne `awaiting` reste à 1. Cette date lointaine garde
        // le schéma SQLite V1 compatible et protège aussi une base restaurée.
        .unwrap_or_else(|| now + chrono::Duration::days(36_500));
    deadline_at
        .map(|deadline| wake_at.min(deadline))
        .unwrap_or(wake_at)
}

fn await_matches_event(run: &FlowRun, event: &EventEnvelope) -> bool {
    run.awaiting
        .as_ref()
        .and_then(|awaiting| {
            serde_json::from_value::<robine_flow_plan::AwaitTrigger>(awaiting.clone()).ok()
        })
        .is_some_and(|trigger| await_trigger_matches_event(&trigger, event))
}

fn await_trigger_matches_event(
    trigger: &robine_flow_plan::AwaitTrigger,
    event: &EventEnvelope,
) -> bool {
    match trigger {
        robine_flow_plan::AwaitTrigger::AnyOf { triggers } => triggers
            .iter()
            .any(|trigger| await_trigger_matches_event(trigger, event)),
        robine_flow_plan::AwaitTrigger::EventType { event_type } => {
            event.data.event_type() == *event_type
        }
        robine_flow_plan::AwaitTrigger::StateChanged { .. } => false,
    }
}

struct HomeCommandGateway<'a> {
    commands: &'a HomeService,
    flows: &'a FlowService,
    now: DateTime<Utc>,
    correlation_id: String,
}
impl CommandGateway for HomeCommandGateway<'_> {
    fn request(
        &self,
        entity_id: &str,
        key: &str,
        value: StateValue,
        idempotency_key: String,
    ) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(entity_id)
            .map_err(|_| "Flow entity reference is not a UUID".to_string())?;
        self.commands
            .request_command_with_correlation(
                EntityId(id),
                key.into(),
                value,
                idempotency_key,
                self.now,
                self.correlation_id.clone(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn set_automation_enabled(
        &self,
        flow_id: &str,
        enabled: bool,
        _idempotency_key: String,
    ) -> Result<(), String> {
        self.flows.set_enabled_from_flow(flow_id, enabled, self.now)
    }
}

struct SimulationGateway;
impl CommandGateway for SimulationGateway {
    fn request(
        &self,
        _entity_id: &str,
        _key: &str,
        _value: StateValue,
        _idempotency_key: String,
    ) -> Result<(), String> {
        Ok(())
    }

    fn set_automation_enabled(
        &self,
        _flow_id: &str,
        _enabled: bool,
        _idempotency_key: String,
    ) -> Result<(), String> {
        Ok(())
    }
}

fn await_matches_state(run: &FlowRun, state: &StateProperty) -> bool {
    run.awaiting
        .as_ref()
        .and_then(|awaiting| {
            serde_json::from_value::<robine_flow_plan::AwaitTrigger>(awaiting.clone()).ok()
        })
        .is_some_and(|trigger| await_trigger_matches_state(&trigger, state))
}

fn await_trigger_matches_state(
    trigger: &robine_flow_plan::AwaitTrigger,
    state: &StateProperty,
) -> bool {
    match trigger {
        robine_flow_plan::AwaitTrigger::AnyOf { triggers } => triggers
            .iter()
            .any(|trigger| await_trigger_matches_state(trigger, state)),
        robine_flow_plan::AwaitTrigger::StateChanged {
            entity_id,
            property,
            to,
        } => {
            entity_id == &state.entity_id.to_string()
                && property == &state.key
                && to
                    .as_ref()
                    .is_none_or(|expected| match (expected, &state.value) {
                        (robine_flow_plan::AwaitValue::Bool(left), StateValue::Bool(right)) => {
                            left == right
                        }
                        (
                            robine_flow_plan::AwaitValue::Percentage(left),
                            StateValue::Percentage(right),
                        ) => (left - right).abs() < f64::EPSILON,
                        (robine_flow_plan::AwaitValue::Text(left), StateValue::Text(right)) => {
                            left == right
                        }
                        _ => false,
                    })
        }
        robine_flow_plan::AwaitTrigger::EventType { .. } => false,
    }
}

fn state_trigger_matches(ast: &FlowAst, state: &StateProperty) -> bool {
    let Some(on) = ast
        .root
        .list()
        .and_then(|forms| {
            forms.iter().find(|section| {
                section
                    .list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("on")
            })
        })
        .and_then(Form::list)
    else {
        return false;
    };
    let Some(trigger) = on.get(1).and_then(Form::list) else {
        return false;
    };
    state_trigger_form_matches(trigger, state)
}

fn schedule_trigger_key(ast: &FlowAst, now: DateTime<Utc>) -> Option<String> {
    let trigger = ast.root.list()?.iter().find_map(|section| {
        let items = section.list()?;
        (items.first().and_then(Form::symbol) == Some("on"))
            .then_some(items.get(1))
            .flatten()
    })?;
    schedule_trigger_form_key(trigger, now, "")
}

fn schedule_trigger_form_key(trigger: &Form, now: DateTime<Utc>, branch: &str) -> Option<String> {
    let items = trigger.list()?;
    if items.first().and_then(Form::symbol) == Some("any-of") {
        return items[1..].iter().enumerate().find_map(|(index, trigger)| {
            schedule_trigger_form_key(trigger, now, &format!("{branch}.{index}"))
        });
    }
    if items.first().and_then(Form::symbol) != Some("schedule") {
        return None;
    }
    let schedule = parse_schedule(items)?;
    let local = now.with_timezone(&schedule.timezone);
    (local.hour() == schedule.hour
        && local.minute() == schedule.minute
        && schedule
            .weekdays
            .as_ref()
            .is_none_or(|weekdays| weekdays.contains(&local.weekday())))
    .then(|| {
        let branch = (!branch.is_empty()).then(|| format!(":{branch}"));
        format!(
            "schedule{}:{}:{}:{}",
            branch.unwrap_or_default(),
            schedule.timezone.name(),
            local.date_naive(),
            schedule.at
        )
    })
}

fn parse_schedule_time(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.split_once(':')?;
    if hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    let hour = hour.parse().ok()?;
    let minute = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

struct Schedule {
    at: String,
    timezone: Tz,
    hour: u32,
    minute: u32,
    weekdays: Option<Vec<Weekday>>,
}

fn parse_schedule(items: &[Form]) -> Option<Schedule> {
    let mut at = None;
    let mut timezone = None;
    let mut weekdays = None;
    for pair in items[1..].chunks(2) {
        let [Form::Keyword(key), value] = pair else {
            return None;
        };
        match key.as_str() {
            "at" if at.is_none() => {
                at = match value {
                    Form::String(value) => Some(value.clone()),
                    _ => return None,
                }
            }
            "timezone" if timezone.is_none() => {
                timezone = match value {
                    Form::String(value) => value.parse::<Tz>().ok(),
                    _ => return None,
                }
            }
            "weekdays" if weekdays.is_none() => {
                let values = value.list()?;
                if values.is_empty() || values.len() > 7 {
                    return None;
                }
                let mut parsed = Vec::with_capacity(values.len());
                for value in values {
                    let weekday = parse_weekday(value)?;
                    if parsed.contains(&weekday) {
                        return None;
                    }
                    parsed.push(weekday);
                }
                weekdays = Some(parsed);
            }
            _ => return None,
        }
    }
    let at = at?;
    let (hour, minute) = parse_schedule_time(&at)?;
    Some(Schedule {
        at,
        timezone: timezone?,
        hour,
        minute,
        weekdays,
    })
}

fn parse_weekday(form: &Form) -> Option<Weekday> {
    let value = match form {
        Form::Symbol(value) | Form::Keyword(value) | Form::String(value) => value.as_str(),
        _ => return None,
    };
    match value {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn state_trigger_form_matches(trigger: &[Form], state: &StateProperty) -> bool {
    if trigger.first().and_then(Form::symbol) == Some("any-of") {
        return trigger[1..]
            .iter()
            .filter_map(Form::list)
            .any(|trigger| state_trigger_form_matches(trigger, state));
    }
    if trigger.first().and_then(Form::symbol) != Some("state-changed") {
        return false;
    }
    let Some(entity) = trigger
        .get(1)
        .and_then(Form::list)
        .and_then(|items| items.get(1))
        .and_then(|form| {
            if let Form::String(id) = form {
                Some(id)
            } else {
                None
            }
        })
    else {
        return false;
    };
    if entity != &state.entity_id.to_string() {
        return false;
    }
    let mut index = 2;
    while index + 1 < trigger.len() {
        if let Form::Keyword(key) = &trigger[index] {
            match key.as_str() {
                "property" => {
                    if !matches!(&trigger[index + 1], Form::String(value) if value == &state.key) {
                        return false;
                    }
                }
                "to" if !form_matches_state_value(&trigger[index + 1], &state.value) => {
                    return false;
                }
                _ => {}
            }
        }
        index += 2;
    }
    true
}

fn event_trigger_matches(ast: &FlowAst, event: &EventEnvelope) -> bool {
    let Some(on) = ast
        .root
        .list()
        .and_then(|forms| {
            forms.iter().find(|section| {
                section
                    .list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("on")
            })
        })
        .and_then(Form::list)
    else {
        return false;
    };
    let Some(trigger) = on.get(1).and_then(Form::list) else {
        return false;
    };
    event_trigger_form_matches(trigger, event)
}

fn event_trigger_form_matches(trigger: &[Form], event: &EventEnvelope) -> bool {
    if trigger.first().and_then(Form::symbol) == Some("any-of") {
        return trigger[1..]
            .iter()
            .filter_map(Form::list)
            .any(|trigger| event_trigger_form_matches(trigger, event));
    }
    if trigger.first().and_then(Form::symbol) != Some("event") {
        return false;
    }
    trigger.windows(2).any(|pair| {
        matches!(&pair[0], Form::Keyword(key) if key == "type")
            && matches!(&pair[1], Form::String(value) if value == event.data.event_type())
    })
}
fn form_matches_state_value(form: &Form, value: &StateValue) -> bool {
    match (form, value) {
        (Form::Bool(left), StateValue::Bool(right)) => left == right,
        (
            Form::Number {
                literal,
                unit: Some(unit),
            },
            StateValue::Percentage(right),
        ) if unit == "%" => literal
            .parse::<f64>()
            .is_ok_and(|left| (left - right).abs() < f64::EPSILON),
        (Form::String(left), StateValue::Text(right)) => left == right,
        _ => false,
    }
}

#[derive(Clone, Debug)]
enum GuardValue {
    Bool(bool),
    Percentage(f64),
    Text(String),
}

/// Évalue la garde sur le snapshot que fournit le repository au démarrage du
/// run. Une valeur manquante rend la condition fausse : elle ne se transforme
/// jamais en zéro, chaîne vide ou vrai implicite.
fn evaluate_guard(ast: &FlowAst, repository: &dyn HomeRepository) -> Result<bool, FlowError> {
    let Some(when) = ast.root.list().and_then(|forms| {
        forms.iter().find(|section| {
            section
                .list()
                .and_then(|items| items.first())
                .and_then(Form::symbol)
                == Some("when")
        })
    }) else {
        return Ok(true);
    };
    let items = when
        .list()
        .ok_or_else(|| guard_error("when must be a list"))?;
    if items.len() != 2 {
        return Err(guard_error("when requires exactly one condition"));
    }
    evaluate_guard_form(&items[1], repository)
}

fn evaluate_guard_form(form: &Form, repository: &dyn HomeRepository) -> Result<bool, FlowError> {
    let items = form
        .list()
        .ok_or_else(|| guard_error("guard must be a boolean expression"))?;
    match items.first().and_then(Form::symbol) {
        Some("all") => {
            if items.len() < 2 {
                return Err(guard_error("all requires at least one condition"));
            }
            items[1..]
                .iter()
                .map(|form| evaluate_guard_form(form, repository))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| values.into_iter().all(|value| value))
        }
        Some("any") => {
            if items.len() < 2 {
                return Err(guard_error("any requires at least one condition"));
            }
            items[1..]
                .iter()
                .map(|form| evaluate_guard_form(form, repository))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| values.into_iter().any(|value| value))
        }
        Some("not") if items.len() == 2 => {
            evaluate_guard_form(&items[1], repository).map(|value| !value)
        }
        Some("available?" | "present?") if items.len() == 2 => {
            guard_value(&items[1], repository).map(|value| value.is_some())
        }
        Some("=" | "!=" | "<" | "<=" | ">" | ">=") if items.len() == 3 => {
            let left = guard_value(&items[1], repository)?;
            let right = guard_value(&items[2], repository)?;
            Ok(
                match (items[0].symbol().expect("guard head exists"), left, right) {
                    (_, None, _) | (_, _, None) => false,
                    ("=", Some(left), Some(right)) => guard_values_equal(&left, &right),
                    ("!=", Some(left), Some(right)) => !guard_values_equal(&left, &right),
                    (
                        operator,
                        Some(GuardValue::Percentage(left)),
                        Some(GuardValue::Percentage(right)),
                    ) => match operator {
                        "<" => left < right,
                        "<=" => left <= right,
                        ">" => left > right,
                        ">=" => left >= right,
                        _ => unreachable!("comparison operator was matched"),
                    },
                    _ => false,
                },
            )
        }
        Some("not") => Err(guard_error("not requires exactly one condition")),
        Some("available?" | "present?") => Err(guard_error(
            "available? and present? require exactly one value",
        )),
        Some(operator) => Err(guard_error(&format!("unsupported guard form {operator}"))),
        None => Err(guard_error("guard form has no symbol")),
    }
}

fn guard_value(
    form: &Form,
    repository: &dyn HomeRepository,
) -> Result<Option<GuardValue>, FlowError> {
    match form {
        Form::Bool(value) => Ok(Some(GuardValue::Bool(*value))),
        Form::String(value) => Ok(Some(GuardValue::Text(value.clone()))),
        Form::Number {
            literal,
            unit: Some(unit),
        } if unit == "%" => literal
            .parse::<f64>()
            .map(GuardValue::Percentage)
            .map(Some)
            .map_err(|_| guard_error("invalid percentage in guard")),
        Form::List(items) if items.first().and_then(Form::symbol) == Some("state") => {
            let entity_id = items
                .get(1)
                .and_then(Form::list)
                .filter(|entity| entity.first().and_then(Form::symbol) == Some("entity"))
                .and_then(|entity| entity.get(1))
                .and_then(|entity| match entity {
                    Form::String(value) => Some(value),
                    _ => None,
                })
                .ok_or_else(|| guard_error("state requires an entity reference"))?;
            let property = items
                .get(2)
                .and_then(|property| match property {
                    Form::Keyword(value) | Form::String(value) => Some(value),
                    _ => None,
                })
                .ok_or_else(|| guard_error("state requires a property"))?;
            if items.len() != 3 {
                return Err(guard_error("state accepts only an entity and one property"));
            }
            let entity_id = uuid::Uuid::parse_str(entity_id)
                .map_err(|_| guard_error("state entity reference is not a UUID"))?;
            let value = repository
                .get_entity_state(&EntityId(entity_id))?
                .into_iter()
                .find(|state| {
                    state.key == *property
                        && matches!(
                            state.quality,
                            StateQuality::Reported | StateQuality::Estimated
                        )
                })
                .map(|state| match state.value {
                    StateValue::Bool(value) => GuardValue::Bool(value),
                    StateValue::Percentage(value) => GuardValue::Percentage(value),
                    StateValue::Text(value) => GuardValue::Text(value),
                });
            Ok(value)
        }
        _ => Err(guard_error("unsupported value in guard")),
    }
}

fn guard_values_equal(left: &GuardValue, right: &GuardValue) -> bool {
    match (left, right) {
        (GuardValue::Bool(left), GuardValue::Bool(right)) => left == right,
        (GuardValue::Percentage(left), GuardValue::Percentage(right)) => {
            (left - right).abs() < f64::EPSILON
        }
        (GuardValue::Text(left), GuardValue::Text(right)) => left == right,
        _ => false,
    }
}

fn guard_error(message: &str) -> FlowError {
    FlowError::Application(ApplicationError::Validation(message.into()))
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
        validate_meta_syntax(&ast)?;
        validate_schedule_syntax(&ast)?;
        validate_guard_syntax(&ast)?;
        validate_choose_syntax(&ast)?;
        validate_automation_actions(&ast, repository)?;
        Ok(ast)
    } else {
        Err(FlowError::Validation(diagnostics))
    }
}

fn validate_automation_actions(
    ast: &FlowAst,
    repository: &dyn HomeRepository,
) -> Result<(), FlowError> {
    fn invalid(message: &str) -> FlowError {
        FlowError::Validation(vec![CheckDiagnostic {
            code: "flow.automation_target",
            message: message.into(),
            path: "$/do".into(),
        }])
    }
    fn visit(form: &Form, repository: &dyn HomeRepository) -> Result<(), FlowError> {
        let Some(items) = form.list() else {
            return Ok(());
        };
        if matches!(
            items.first().and_then(Form::symbol),
            Some("activate" | "deactivate")
        ) {
            let target = items
                .get(1)
                .and_then(Form::list)
                .filter(|target| target.first().and_then(Form::symbol) == Some("flow"))
                .and_then(|target| target.get(1))
                .and_then(|target| match target {
                    Form::String(value) => Some(value),
                    _ => None,
                })
                .ok_or_else(|| invalid("activate/deactivate requires (flow \"uuid\")"))?;
            let id = uuid::Uuid::parse_str(target)
                .map(FlowId)
                .map_err(|_| invalid("automation reference must be a UUID"))?;
            if repository
                .get_flow(&id)
                .map_err(FlowError::Application)?
                .is_none()
            {
                return Err(invalid("referenced automation does not exist"));
            }
        }
        items.iter().try_for_each(|child| visit(child, repository))
    }
    visit(&ast.root, repository)
}

fn validate_schedule_syntax(ast: &FlowAst) -> Result<(), FlowError> {
    let Some(trigger) = ast.root.list().and_then(|forms| {
        forms.iter().find_map(|section| {
            let items = section.list()?;
            (items.first().and_then(Form::symbol) == Some("on"))
                .then_some(items.get(1))
                .flatten()
        })
    }) else {
        return Ok(());
    };
    fn validate(trigger: &Form) -> bool {
        let Some(items) = trigger.list() else {
            return false;
        };
        match items.first().and_then(Form::symbol) {
            Some("schedule") => matches!(items.len(), 5 | 7) && parse_schedule(items).is_some(),
            Some("any-of") => {
                items.len() >= 3
                    && items[1..].iter().all(|branch| {
                        let Some(branch_items) = branch.list() else {
                            return false;
                        };
                        match branch_items.first().and_then(Form::symbol) {
                            Some("schedule" | "any-of") => validate(branch),
                            Some("event" | "state-changed") => true,
                            _ => false,
                        }
                    })
            }
            Some("event" | "state-changed") => true,
            _ => false,
        }
    }
    validate(trigger).then_some(()).ok_or_else(|| FlowError::Validation(vec![CheckDiagnostic {
        code: "flow.schedule",
        message: "schedule requires :at \"HH:MM\", an IANA :timezone and optional :weekdays [mon ... sun]".into(),
        path: "$/on".into(),
    }]))
}

fn validate_choose_syntax(ast: &FlowAst) -> Result<(), FlowError> {
    fn visit(form: &Form) -> Result<(), FlowError> {
        let Some(items) = form.list() else {
            return Ok(());
        };
        if items.first().and_then(Form::symbol) == Some("choose") {
            if !(3..=4).contains(&items.len()) {
                return Err(invalid_choose_diagnostic(
                    "choose requires a predicate, a then branch and an optional else branch",
                ));
            }
            validate_guard_form_syntax(&items[1]).map_err(|_| {
                invalid_choose_diagnostic("choose predicate is not a supported boolean expression")
            })?;
            for branch in &items[2..] {
                let Some(branch_items) = branch.list() else {
                    return Err(invalid_choose_diagnostic(
                        "choose branches must be do blocks",
                    ));
                };
                if branch_items.first().and_then(Form::symbol) != Some("do") {
                    return Err(invalid_choose_diagnostic(
                        "choose branches must be do blocks",
                    ));
                }
            }
        }
        items.iter().try_for_each(visit)
    }
    visit(&ast.root)
}

fn invalid_choose_diagnostic(message: &str) -> FlowError {
    FlowError::Validation(vec![CheckDiagnostic {
        code: "flow.choose",
        message: message.into(),
        path: "$/do".into(),
    }])
}

/// Fige la branche retenue dans l'AST avant sa compilation. Le plan persisté
/// ne contient donc plus de condition mutable : après un `wait` ou un
/// redémarrage, une exécution reprend exactement la branche initiale.
fn resolve_choose_actions(
    ast: &FlowAst,
    repository: &dyn HomeRepository,
) -> Result<FlowAst, FlowError> {
    fn resolve_sequence(
        actions: &[Form],
        repository: &dyn HomeRepository,
    ) -> Result<Vec<Form>, FlowError> {
        let mut resolved = Vec::new();
        for action in actions {
            let Some(items) = action.list() else {
                return Err(invalid_choose_diagnostic("an action must be a list"));
            };
            if items.first().and_then(Form::symbol) != Some("choose") {
                resolved.push(action.clone());
                continue;
            }
            let selected_then = evaluate_guard_form(&items[1], repository)?;
            resolved.push(Form::List(vec![
                Form::Symbol("audit".into()),
                Form::Keyword("message".into()),
                Form::String(
                    if selected_then {
                        "choose: then"
                    } else {
                        "choose: else"
                    }
                    .into(),
                ),
            ]));
            let branch = if selected_then {
                items.get(2)
            } else {
                items.get(3)
            };
            if let Some(branch) = branch {
                let branch_items = branch
                    .list()
                    .ok_or_else(|| invalid_choose_diagnostic("choose branch must be a do block"))?;
                resolved.extend(resolve_sequence(&branch_items[1..], repository)?);
            }
        }
        Ok(resolved)
    }

    let mut root = ast.root.clone();
    let forms = root
        .list()
        .ok_or_else(|| invalid_choose_diagnostic("flow root must be a list"))?
        .to_vec();
    let mut rewritten = Vec::with_capacity(forms.len());
    for form in forms {
        if form
            .list()
            .and_then(|items| items.first())
            .and_then(Form::symbol)
            == Some("do")
        {
            let items = form.list().expect("checked list");
            let mut body = vec![Form::Symbol("do".into())];
            body.extend(resolve_sequence(&items[1..], repository)?);
            rewritten.push(Form::List(body));
        } else {
            rewritten.push(form);
        }
    }
    root = Form::List(rewritten);
    FlowAst::new(root)
        .map_err(|error| FlowError::Application(ApplicationError::Validation(error.to_string())))
}

fn validate_meta_syntax(ast: &FlowAst) -> Result<(), FlowError> {
    let Some(meta) = ast
        .root
        .list()
        .and_then(|forms| {
            forms.iter().find(|section| {
                section
                    .list()
                    .and_then(|items| items.first())
                    .and_then(Form::symbol)
                    == Some("meta")
            })
        })
        .and_then(Form::list)
    else {
        return Ok(());
    };
    if meta.len() % 2 == 0 {
        return Err(invalid_meta_diagnostic("meta requires keyword/value pairs"));
    }
    let mut seen = std::collections::HashSet::new();
    for pair in meta[1..].chunks(2) {
        let Form::Keyword(key) = &pair[0] else {
            return Err(invalid_meta_diagnostic("meta keys must be keywords"));
        };
        if !seen.insert(key) {
            return Err(invalid_meta_diagnostic("meta keys must not be repeated"));
        }
        let valid = match key.as_str() {
            "name" | "description" => matches!(pair[1], Form::String(_)),
            "mode" => {
                matches!(&pair[1], Form::Keyword(value) if matches!(value.as_str(), "single" | "restart" | "queue"))
            }
            "ignore-self" | "enabled" => matches!(pair[1], Form::Bool(_)),
            "max-runs" => {
                matches!(&pair[1], Form::Number { literal, unit: None } if literal.parse::<u8>().is_ok_and(|value| (1..=32).contains(&value)))
            }
            "max-runtime" => {
                matches!(&pair[1], Form::Number { literal, unit: Some(unit) } if matches!(unit.as_str(), "ms" | "s" | "m" | "h" | "d") && literal.parse::<f64>().is_ok_and(|value| value > 0.0))
            }
            _ => false,
        };
        if !valid {
            return Err(invalid_meta_diagnostic("invalid or unsupported meta value"));
        }
    }
    Ok(())
}

fn invalid_meta_diagnostic(message: &str) -> FlowError {
    FlowError::Validation(vec![CheckDiagnostic {
        code: "flow.meta",
        message: message.into(),
        path: "$/meta".into(),
    }])
}

fn validate_guard_syntax(ast: &FlowAst) -> Result<(), FlowError> {
    let Some(when) = ast.root.list().and_then(|forms| {
        forms.iter().find(|section| {
            section
                .list()
                .and_then(|items| items.first())
                .and_then(Form::symbol)
                == Some("when")
        })
    }) else {
        return Ok(());
    };
    let items = when
        .list()
        .ok_or_else(|| invalid_guard_diagnostic("when must be a list"))?;
    if items.len() != 2 {
        return Err(invalid_guard_diagnostic(
            "when requires exactly one condition",
        ));
    }
    validate_guard_form_syntax(&items[1])
}

fn validate_guard_form_syntax(form: &Form) -> Result<(), FlowError> {
    let items = form
        .list()
        .ok_or_else(|| invalid_guard_diagnostic("guard must be a boolean expression"))?;
    match items.first().and_then(Form::symbol) {
        Some("all" | "any") if items.len() >= 2 => {
            items[1..].iter().try_for_each(validate_guard_form_syntax)
        }
        Some("not") if items.len() == 2 => validate_guard_form_syntax(&items[1]),
        Some("available?" | "present?") if items.len() == 2 => {
            validate_guard_value_syntax(&items[1])
        }
        Some("=" | "!=" | "<" | "<=" | ">" | ">=") if items.len() == 3 => {
            validate_guard_value_syntax(&items[1])?;
            validate_guard_value_syntax(&items[2])
        }
        Some("all" | "any") => Err(invalid_guard_diagnostic(
            "all and any require at least one condition",
        )),
        Some("not") => Err(invalid_guard_diagnostic(
            "not requires exactly one condition",
        )),
        Some("available?" | "present?") => Err(invalid_guard_diagnostic(
            "available? and present? require exactly one value",
        )),
        Some(operator) => Err(invalid_guard_diagnostic(&format!(
            "unsupported guard form {operator}"
        ))),
        None => Err(invalid_guard_diagnostic("guard form has no symbol")),
    }
}

fn validate_guard_value_syntax(form: &Form) -> Result<(), FlowError> {
    match form {
        Form::Bool(_) | Form::String(_) => Ok(()),
        Form::Number {
            literal,
            unit: Some(unit),
        } if unit == "%" && literal.parse::<f64>().is_ok() => Ok(()),
        Form::List(items) if items.first().and_then(Form::symbol) == Some("state") => {
            let entity = items
                .get(1)
                .and_then(Form::list)
                .filter(|entity| entity.first().and_then(Form::symbol) == Some("entity"))
                .and_then(|entity| entity.get(1))
                .and_then(|entity| match entity {
                    Form::String(value) => Some(value),
                    _ => None,
                })
                .filter(|entity| uuid::Uuid::parse_str(entity).is_ok());
            let property = items.get(2).is_some_and(|property| {
                matches!(property, Form::Keyword(value) | Form::String(value) if !value.is_empty())
            });
            if items.len() == 3 && entity.is_some() && property {
                Ok(())
            } else {
                Err(invalid_guard_diagnostic(
                    "state requires a UUID entity reference and one property",
                ))
            }
        }
        _ => Err(invalid_guard_diagnostic("unsupported value in guard")),
    }
}

fn invalid_guard_diagnostic(message: &str) -> FlowError {
    FlowError::Validation(vec![CheckDiagnostic {
        code: "flow.guard_unsupported",
        message: message.into(),
        path: "$/when".into(),
    }])
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
#[derive(Clone, Debug, serde::Serialize)]
pub struct FlowSimulation {
    pub name: Option<String>,
    pub command_count: usize,
    pub diagnostics: Vec<CheckDiagnostic>,
    pub result: RunResult,
}

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("Flow syntax is invalid")]
    Syntax(Vec<robine_flow_syntax::Diagnostic>),
    #[error("Flow validation failed")]
    Validation(Vec<CheckDiagnostic>),
    #[error("automation not found")]
    NotFound,
    #[error("automation is disabled")]
    Disabled,
    #[error("this automation already consumed the causal event")]
    AlreadyConsumed,
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
    pub fn list_devices_page(
        &self,
        request: DevicePageRequest,
    ) -> Result<DevicePage, ApplicationError> {
        self.repository.list_devices_page(request)
    }
    pub fn rename_device(
        &self,
        id: DeviceId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<Device, ApplicationError> {
        validate_display_name(&name)?;
        let (device, event) = self.repository.rename_device(&id, name, now)?;
        self.events.publish(event);
        Ok(device)
    }
    pub fn remove_device(
        &self,
        id: DeviceId,
        now: DateTime<Utc>,
    ) -> Result<Device, ApplicationError> {
        let (device, event) = self.repository.remove_device(&id, now)?;
        self.events.publish(event);
        Ok(device)
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
    pub fn rename_entity(
        &self,
        id: EntityId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<Entity, ApplicationError> {
        validate_display_name(&name)?;
        let (entity, event) = self.repository.rename_entity(&id, name, now)?;
        self.events.publish(event);
        Ok(entity)
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
    pub fn assign_entity_area(
        &self,
        entity_id: EntityId,
        area_id: Option<AreaId>,
        now: DateTime<Utc>,
    ) -> Result<Entity, ApplicationError> {
        let (entity, event) =
            self.repository
                .assign_entity_area(&entity_id, area_id.as_ref(), now)?;
        self.events.publish(event);
        Ok(entity)
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
        let entity = self
            .repository
            .get_entity(&state.entity_id)?
            .ok_or(ApplicationError::EntityNotFound)?;
        if !entity.supports(&state.key) {
            return Err(ApplicationError::CapabilityNotSupported(state.key));
        }
        if !state.value.is_valid_for(&state.key) {
            return Err(ApplicationError::Domain(DomainError::InvalidStateValue(
                state.key,
            )));
        }
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
        self.request_command_with_correlation(
            entity_id,
            key,
            value,
            idempotency_key,
            now,
            format!("cor_{}", uuid::Uuid::new_v4()),
        )
    }

    fn request_command_with_correlation(
        &self,
        entity_id: EntityId,
        key: String,
        value: StateValue,
        idempotency_key: String,
        now: DateTime<Utc>,
        correlation_id: String,
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
        if !self.repository.is_entity_commandable(&entity_id)? {
            return Err(ApplicationError::Validation(
                "the entity belongs to a removed device".into(),
            ));
        }
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
            correlation_id,
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
    pub fn recent_events(&self, limit: usize) -> Result<Vec<EventEnvelope>, ApplicationError> {
        self.repository.recent_events(limit)
    }
    pub fn latest_event_sequence(&self) -> Result<u64, ApplicationError> {
        self.repository.latest_event_sequence()
    }
    pub fn expire_stale_commands(
        &self,
        now: DateTime<Utc>,
        timeout: chrono::Duration,
    ) -> Result<usize, ApplicationError> {
        let events = self.repository.expire_commands(now - timeout, now, 100)?;
        let count = events.len();
        for event in events {
            self.events.publish(event);
        }
        Ok(count)
    }
}

fn validate_display_name(name: &str) -> Result<(), ApplicationError> {
    if name.trim().is_empty() || name.chars().count() > 120 {
        return Err(ApplicationError::Validation(
            "name must contain between 1 and 120 characters".into(),
        ));
    }
    Ok(())
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

#[cfg(test)]
mod flow_meta_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn meta_rejects_unknown_or_repeated_execution_options() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("meta".into()),
                Form::Keyword("max-runs".into()),
                Form::Number {
                    literal: "2".into(),
                    unit: None,
                },
                Form::Keyword("max-runs".into()),
                Form::Number {
                    literal: "3".into(),
                    unit: None,
                },
            ]),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![Form::Symbol("event".into())]),
            ]),
            Form::List(vec![Form::Symbol("do".into())]),
        ]))
        .unwrap();
        assert!(
            matches!(validate_meta_syntax(&flow), Err(FlowError::Validation(diagnostics)) if diagnostics[0].code == "flow.meta")
        );
    }

    #[test]
    fn schedule_matches_its_local_minute_once_with_a_stable_correlation_key() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![
                    Form::Symbol("schedule".into()),
                    Form::Keyword("at".into()),
                    Form::String("09:15".into()),
                    Form::Keyword("timezone".into()),
                    Form::String("Europe/Paris".into()),
                ]),
            ]),
            Form::List(vec![Form::Symbol("do".into())]),
        ]))
        .unwrap();
        let matching = Utc.with_ymd_and_hms(2026, 1, 15, 8, 15, 42).unwrap();
        assert_eq!(
            schedule_trigger_key(&flow, matching),
            Some("schedule:Europe/Paris:2026-01-15:09:15".into())
        );
        assert!(
            schedule_trigger_key(&flow, Utc.with_ymd_and_hms(2026, 1, 15, 8, 16, 0).unwrap())
                .is_none()
        );
    }

    #[test]
    fn schedule_rejects_an_invalid_clock_or_timezone() {
        let flow = FlowAst::new(Form::List(vec![
            Form::Symbol("flow".into()),
            Form::List(vec![
                Form::Symbol("on".into()),
                Form::List(vec![
                    Form::Symbol("schedule".into()),
                    Form::Keyword("at".into()),
                    Form::String("25:90".into()),
                    Form::Keyword("timezone".into()),
                    Form::String("Not/AZone".into()),
                ]),
            ]),
            Form::List(vec![Form::Symbol("do".into())]),
        ]))
        .unwrap();
        assert!(matches!(
            validate_schedule_syntax(&flow),
            Err(FlowError::Validation(diagnostics)) if diagnostics[0].code == "flow.schedule"
        ));
    }
}
