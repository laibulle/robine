//! Adaptateur Matter : le SDK, la fabric et le RPC restent derrière `MatterClient`.

use chrono::{DateTime, Utc};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::*;
use robine_matter_contract::{
    AttributeValue, Cluster, ClusterCommand, CommissioningJob, Endpoint, MatterEvent,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;

pub const ADAPTER_ID: &str = "matter:local";

pub trait MatterClient: Send + Sync {
    fn health(&self) -> Result<bool, MatterError>;
    fn list_endpoints(&self) -> Result<Vec<Endpoint>, MatterError>;
    fn invoke(
        &self,
        fabric_id: &str,
        node_id: &str,
        endpoint_id: u16,
        command: ClusterCommand,
    ) -> Result<(), MatterError>;
    fn start_commissioning(&self, setup_code: &str) -> Result<String, MatterError>;
    fn commissioning_job(&self, job_id: &str) -> Result<CommissioningJob, MatterError>;
}

#[derive(Debug, Error)]
pub enum MatterError {
    #[error("Matter sidecar is unavailable: {0}")]
    Unavailable(String),
    #[error("unsupported Matter capability")]
    Unsupported,
    #[error("Matter local RPC protocol error: {0}")]
    Protocol(String),
}

/// Client JSON-lines du socket Unix privé du sidecar. Une connexion par appel
/// évite qu'un sidecar mort conserve un descripteur ambigu ; les timeouts
/// protègent les workers Actix qui invoquent cet adaptateur en tâche bloquante.
pub struct LocalMatterClient {
    socket_path: PathBuf,
    authorization: String,
    timeout: Duration,
}

impl LocalMatterClient {
    pub fn new(
        socket_path: PathBuf,
        authorization: String,
        timeout: Duration,
    ) -> Result<Self, MatterError> {
        if authorization.trim().is_empty() {
            return Err(MatterError::Protocol(
                "local RPC authorization is empty".into(),
            ));
        }
        Ok(Self {
            socket_path,
            authorization,
            timeout,
        })
    }

    fn call(
        &self,
        body: robine_matter_contract::MatterRequest,
    ) -> Result<robine_matter_contract::MatterResponse, MatterError> {
        use robine_matter_contract::{
            AuthenticatedRpcRequest, RPC_VERSION, RpcRequest, RpcResponse,
        };
        let request_id = uuid::Uuid::new_v4().to_string();
        let request = AuthenticatedRpcRequest {
            authorization: self.authorization.clone(),
            request: RpcRequest {
                rpc_version: RPC_VERSION,
                request_id: request_id.clone(),
                body,
            },
        };
        request
            .validate()
            .map_err(|error| MatterError::Protocol(error.to_string()))?;
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|error| MatterError::Unavailable(error.to_string()))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|error| MatterError::Unavailable(error.to_string()))?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|error| MatterError::Unavailable(error.to_string()))?;
        let encoded = serde_json::to_vec(&request)
            .map_err(|error| MatterError::Protocol(error.to_string()))?;
        stream
            .write_all(&encoded)
            .and_then(|_| stream.write_all(b"\n"))
            .and_then(|_| stream.flush())
            .map_err(|error| MatterError::Unavailable(error.to_string()))?;
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .map_err(|error| MatterError::Unavailable(error.to_string()))?;
        if response.len() > 1_048_576 {
            return Err(MatterError::Protocol("response exceeds 1 MiB".into()));
        }
        let response: RpcResponse<robine_matter_contract::MatterResponse> =
            serde_json::from_str(&response)
                .map_err(|error| MatterError::Protocol(error.to_string()))?;
        if response.rpc_version != RPC_VERSION || response.request_id != request_id {
            return Err(MatterError::Protocol(
                "response version or request id does not match".into(),
            ));
        }
        Ok(response.body)
    }
}

impl MatterClient for LocalMatterClient {
    fn health(&self) -> Result<bool, MatterError> {
        match self.call(robine_matter_contract::MatterRequest::Health)? {
            robine_matter_contract::MatterResponse::Health { available, .. } => Ok(available),
            _ => Err(MatterError::Protocol(
                "unexpected response to health".into(),
            )),
        }
    }
    fn list_endpoints(&self) -> Result<Vec<Endpoint>, MatterError> {
        match self.call(robine_matter_contract::MatterRequest::ListEndpoints)? {
            robine_matter_contract::MatterResponse::Endpoints { endpoints } => Ok(endpoints),
            _ => Err(MatterError::Protocol(
                "unexpected response to endpoint listing".into(),
            )),
        }
    }
    fn invoke(
        &self,
        fabric_id: &str,
        node_id: &str,
        endpoint_id: u16,
        command: ClusterCommand,
    ) -> Result<(), MatterError> {
        match self.call(robine_matter_contract::MatterRequest::Invoke {
            fabric_id: fabric_id.into(),
            node_id: node_id.into(),
            endpoint_id,
            command,
        })? {
            robine_matter_contract::MatterResponse::InvocationAccepted { .. } => Ok(()),
            _ => Err(MatterError::Protocol(
                "unexpected response to invocation".into(),
            )),
        }
    }
    fn start_commissioning(&self, setup_code: &str) -> Result<String, MatterError> {
        match self.call(robine_matter_contract::MatterRequest::StartCommissioning {
            setup_code: setup_code.into(),
        })? {
            robine_matter_contract::MatterResponse::CommissioningStarted { job_id } => Ok(job_id),
            robine_matter_contract::MatterResponse::Error { detail, .. } => {
                Err(MatterError::Protocol(detail))
            }
            _ => Err(MatterError::Protocol(
                "unexpected response to commissioning start".into(),
            )),
        }
    }
    fn commissioning_job(&self, job_id: &str) -> Result<CommissioningJob, MatterError> {
        match self.call(robine_matter_contract::MatterRequest::GetJob {
            job_id: job_id.into(),
        })? {
            robine_matter_contract::MatterResponse::Job { job } => Ok(job),
            robine_matter_contract::MatterResponse::Error { detail, .. } => {
                Err(MatterError::Protocol(detail))
            }
            _ => Err(MatterError::Protocol(
                "unexpected response to commissioning job lookup".into(),
            )),
        }
    }
}

#[derive(Clone)]
struct Route {
    fabric_id: String,
    node_id: String,
    endpoint_id: u16,
}

pub struct MatterCommandDispatcher<C: MatterClient> {
    client: Arc<C>,
    routes: Mutex<HashMap<EntityId, Route>>,
}
impl<C: MatterClient> MatterCommandDispatcher<C> {
    pub fn new(client: Arc<C>) -> Self {
        Self {
            client,
            routes: Mutex::new(HashMap::new()),
        }
    }
    fn register(&self, entity: EntityId, route: Route) {
        self.routes
            .lock()
            .expect("Matter route lock poisoned")
            .insert(entity, route);
    }
}
impl<C: MatterClient> CommandDispatcher for MatterCommandDispatcher<C> {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let route = self
            .routes
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("Matter route lock poisoned".into()))?
            .get(&command.entity_id)
            .cloned()
            .ok_or_else(|| ApplicationError::Infrastructure("no Matter route for entity".into()))?;
        let invocation = match (command.key.as_str(), command.value) {
            ("switch", StateValue::Bool(on)) => ClusterCommand::SetOnOff { on },
            ("light.brightness", StateValue::Percentage(percent)) => ClusterCommand::SetLevel {
                percent: percent.round().clamp(0.0, 100.0) as u8,
            },
            ("light.color_temperature", StateValue::Text(mirek)) => {
                ClusterCommand::SetColorTemperature {
                    mirek: mirek.parse().map_err(|_| {
                        ApplicationError::Validation(
                            "light.color_temperature must be a Matter mired value".into(),
                        )
                    })?,
                }
            }
            _ => {
                return Err(ApplicationError::Infrastructure(
                    MatterError::Unsupported.to_string(),
                ));
            }
        };
        self.client
            .invoke(
                &route.fabric_id,
                &route.node_id,
                route.endpoint_id,
                invocation,
            )
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))
    }
}

pub struct MatterAdapter<C: MatterClient> {
    client: Arc<C>,
    service: HomeService,
    dispatcher: Arc<MatterCommandDispatcher<C>>,
    routes: Mutex<HashMap<(String, String, u16), EntityId>>,
}
impl<C: MatterClient> MatterAdapter<C> {
    pub fn new(
        client: Arc<C>,
        service: HomeService,
        dispatcher: Arc<MatterCommandDispatcher<C>>,
    ) -> Self {
        Self {
            client,
            service,
            dispatcher,
            routes: Mutex::new(HashMap::new()),
        }
    }

    pub fn synchronize(&self, now: DateTime<Utc>) -> Result<Vec<Device>, ApplicationError> {
        if !self
            .client
            .health()
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?
        {
            self.service.update_adapter_health(AdapterHealth {
                adapter_id: AdapterId::new(ADAPTER_ID)?,
                status: AdapterStatus::Degraded,
                detail: Some("Matter sidecar reports unavailable".into()),
                observed_at: now,
            })?;
            return Ok(Vec::new());
        }
        let endpoints = self
            .client
            .list_endpoints()
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        let mut grouped: HashMap<(String, String), Vec<Endpoint>> = HashMap::new();
        for endpoint in endpoints {
            grouped
                .entry((endpoint.fabric_id.clone(), endpoint.node_id.clone()))
                .or_default()
                .push(endpoint);
        }
        let mut devices = Vec::new();
        for ((fabric_id, node_id), endpoints) in grouped {
            let discovery = DeviceDiscovery {
                adapter_id: AdapterId::new(ADAPTER_ID)?,
                protocol_address: format!("{fabric_id}/{node_id}"),
                name: endpoints
                    .first()
                    .map(|endpoint| endpoint.name.clone())
                    .unwrap_or_else(|| "Appareil Matter".into()),
                entities: endpoints
                    .iter()
                    .map(|endpoint| {
                        Ok::<_, ApplicationError>(DiscoveryEntity {
                            protocol_address: endpoint.endpoint_id.to_string(),
                            name: endpoint.name.clone(),
                            kind: endpoint_kind(&endpoint.clusters).into(),
                            capabilities: endpoint_capabilities(&endpoint.clusters)?,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            };
            let device = self.service.register_discovery(discovery, now)?;
            for (endpoint, entity) in endpoints.iter().zip(&device.entities) {
                let route = Route {
                    fabric_id: fabric_id.clone(),
                    node_id: node_id.clone(),
                    endpoint_id: endpoint.endpoint_id,
                };
                self.routes
                    .lock()
                    .map_err(|_| {
                        ApplicationError::Infrastructure("Matter route lock poisoned".into())
                    })?
                    .insert(
                        (fabric_id.clone(), node_id.clone(), endpoint.endpoint_id),
                        entity.id.clone(),
                    );
                self.dispatcher.register(entity.id.clone(), route);
            }
            devices.push(device);
        }
        self.service.update_adapter_health(AdapterHealth {
            adapter_id: AdapterId::new(ADAPTER_ID)?,
            status: AdapterStatus::Available,
            detail: None,
            observed_at: now,
        })?;
        Ok(devices)
    }

    pub fn apply_event(
        &self,
        event: MatterEvent,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        match event {
            MatterEvent::AttributeReported {
                fabric_id,
                node_id,
                endpoint_id,
                attribute,
            } => {
                let entity_id = self
                    .routes
                    .lock()
                    .map_err(|_| {
                        ApplicationError::Infrastructure("Matter route lock poisoned".into())
                    })?
                    .get(&(fabric_id, node_id, endpoint_id))
                    .cloned()
                    .ok_or_else(|| {
                        ApplicationError::Infrastructure("unknown Matter endpoint event".into())
                    })?;
                let (key, value) = match attribute {
                    AttributeValue::OnOff { on } => ("switch".into(), StateValue::Bool(on)),
                    AttributeValue::Level { percent } => (
                        "light.brightness".into(),
                        StateValue::Percentage(f64::from(percent)),
                    ),
                    AttributeValue::ColorTemperature { mirek } => (
                        "light.color_temperature".into(),
                        StateValue::Text(mirek.to_string()),
                    ),
                    AttributeValue::Temperature { centi_celsius } => (
                        "sensor.temperature".into(),
                        StateValue::Text(format!("{}", f64::from(centi_celsius) / 100.0)),
                    ),
                    AttributeValue::Humidity { centi_percent } => (
                        "sensor.humidity".into(),
                        StateValue::Percentage(f64::from(centi_percent) / 100.0),
                    ),
                    AttributeValue::Occupancy { occupied } => {
                        ("sensor.occupancy".into(), StateValue::Bool(occupied))
                    }
                };
                self.service.apply_reported_state(
                    ReportedState {
                        entity_id,
                        key,
                        value,
                        source_at: now,
                    },
                    now,
                )
            }
            MatterEvent::AvailabilityChanged { available, .. } => {
                self.service.update_adapter_health(AdapterHealth {
                    adapter_id: AdapterId::new(ADAPTER_ID)?,
                    status: if available {
                        AdapterStatus::Available
                    } else {
                        AdapterStatus::Degraded
                    },
                    detail: None,
                    observed_at: now,
                })
            }
            MatterEvent::JobProgress { .. } => Ok(()),
        }
    }
}

fn endpoint_kind(clusters: &[Cluster]) -> &'static str {
    if clusters.contains(&Cluster::OnOff) {
        "switch"
    } else if clusters.contains(&Cluster::TemperatureMeasurement) {
        "sensor"
    } else {
        "matter-endpoint"
    }
}
fn endpoint_capabilities(clusters: &[Cluster]) -> Result<Vec<Capability>, ApplicationError> {
    let mut keys = Vec::new();
    if clusters.contains(&Cluster::OnOff) {
        keys.push("switch");
    }
    if clusters.contains(&Cluster::LevelControl) {
        keys.push("light.brightness");
    }
    if clusters.contains(&Cluster::ColorControl) {
        keys.push("light.color_temperature");
    }
    if clusters.contains(&Cluster::TemperatureMeasurement) {
        keys.push("sensor.temperature");
    }
    if clusters.contains(&Cluster::RelativeHumidityMeasurement) {
        keys.push("sensor.humidity");
    }
    if clusters.contains(&Cluster::OccupancySensing) || clusters.contains(&Cluster::BooleanState) {
        keys.push("sensor.occupancy");
    }
    keys.into_iter()
        .map(|key| Capability::new(key, 1).map_err(ApplicationError::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::HomeRepository;
    use robine_matter_contract::{
        AuthenticatedRpcRequest, MatterRequest, MatterResponse, RPC_VERSION, RpcResponse,
    };
    use robine_store_sqlite::SqliteStore;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
    };

    struct Fake {
        endpoints: Vec<Endpoint>,
        invoked: Mutex<Vec<ClusterCommand>>,
    }
    impl MatterClient for Fake {
        fn health(&self) -> Result<bool, MatterError> {
            Ok(true)
        }
        fn list_endpoints(&self) -> Result<Vec<Endpoint>, MatterError> {
            Ok(self.endpoints.clone())
        }
        fn invoke(
            &self,
            _: &str,
            _: &str,
            _: u16,
            command: ClusterCommand,
        ) -> Result<(), MatterError> {
            self.invoked.lock().unwrap().push(command);
            Ok(())
        }
        fn start_commissioning(&self, _: &str) -> Result<String, MatterError> {
            Err(MatterError::Unsupported)
        }
        fn commissioning_job(&self, _: &str) -> Result<CommissioningJob, MatterError> {
            Err(MatterError::Unsupported)
        }
    }
    #[test]
    fn maps_supported_endpoint_and_routes_command_and_report() {
        let client = Arc::new(Fake {
            endpoints: vec![Endpoint {
                fabric_id: "fab".into(),
                node_id: "node".into(),
                endpoint_id: 1,
                name: "Lampe".into(),
                clusters: vec![Cluster::OnOff, Cluster::LevelControl, Cluster::ColorControl],
            }],
            invoked: Mutex::new(Vec::new()),
        });
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(MatterCommandDispatcher::new(client.clone()));
        let service = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let adapter = MatterAdapter::new(client.clone(), service.clone(), dispatcher);
        let device = adapter.synchronize(Utc::now()).unwrap().pop().unwrap();
        let entity = device.entities[0].id.clone();
        service
            .request_command(
                entity.clone(),
                "switch".into(),
                StateValue::Bool(true),
                "matter-test".into(),
                Utc::now(),
            )
            .unwrap();
        service
            .request_command(
                entity.clone(),
                "light.color_temperature".into(),
                StateValue::Text("250".into()),
                "matter-color-temperature".into(),
                Utc::now(),
            )
            .unwrap();
        adapter
            .apply_event(
                MatterEvent::AttributeReported {
                    fabric_id: "fab".into(),
                    node_id: "node".into(),
                    endpoint_id: 1,
                    attribute: AttributeValue::ColorTemperature { mirek: 250 },
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            *client.invoked.lock().unwrap(),
            vec![
                ClusterCommand::SetOnOff { on: true },
                ClusterCommand::SetColorTemperature { mirek: 250 },
            ]
        );
        assert_eq!(store.get_entity_state(&entity).unwrap().len(), 1);
    }

    #[test]
    fn local_client_authenticates_and_correlates_a_health_rpc() {
        // Les sockets Unix sont limités à environ 104 octets sur macOS.
        let path = PathBuf::from(format!("/tmp/rm-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: AuthenticatedRpcRequest<MatterRequest> =
                serde_json::from_str(&line).unwrap();
            assert_eq!(request.authorization, "test-token");
            assert!(matches!(request.request.body, MatterRequest::Health));
            let response = RpcResponse {
                rpc_version: RPC_VERSION,
                request_id: request.request.request_id,
                body: MatterResponse::Health {
                    available: true,
                    detail: None,
                },
            };
            let mut stream = stream;
            writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
        });
        let client =
            LocalMatterClient::new(path.clone(), "test-token".into(), Duration::from_secs(1))
                .unwrap();
        assert!(client.health().unwrap());
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
