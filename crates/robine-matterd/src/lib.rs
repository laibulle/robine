//! Daemon local isolant la future stack Matter du serveur Robine.
//!
//! Cette crate ne fournit volontairement pas de pseudo-implémentation de
//! Matter. Un backend concret doit être branché après validation matérielle ;
//! le daemon reste néanmoins un vrai serveur RPC, testable avec un backend
//! déterministe et capable d'orchestrer les jobs de commissioning.

use robine_matter_contract::{
    AuthenticatedRpcRequest, CommissioningJob, JobStatus, MatterRequest, MatterResponse,
    RPC_VERSION, RpcResponse,
};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub trait MatterBackend: Send + Sync {
    fn health(&self) -> Result<(bool, Option<String>), String>;
    fn list_endpoints(&self) -> Result<Vec<robine_matter_contract::Endpoint>, String>;
    fn invoke(
        &self,
        fabric_id: &str,
        node_id: &str,
        endpoint_id: u16,
        command: robine_matter_contract::ClusterCommand,
    ) -> Result<(), String>;
    fn commission(&self, setup_code: &str) -> Result<(), String>;
}

/// Backend sûr par défaut : il expose l'indisponibilité au lieu de simuler un
/// contrôleur Matter ou de conserver des secrets dans Robine.
pub struct UnavailableBackend;
impl MatterBackend for UnavailableBackend {
    fn health(&self) -> Result<(bool, Option<String>), String> {
        Ok((
            false,
            Some("no Matter controller backend is configured".into()),
        ))
    }
    fn list_endpoints(&self) -> Result<Vec<robine_matter_contract::Endpoint>, String> {
        Ok(Vec::new())
    }
    fn invoke(
        &self,
        _: &str,
        _: &str,
        _: u16,
        _: robine_matter_contract::ClusterCommand,
    ) -> Result<(), String> {
        Err("no Matter controller backend is configured".into())
    }
    fn commission(&self, _: &str) -> Result<(), String> {
        Err("no Matter controller backend is configured".into())
    }
}

pub struct MatterDaemon {
    authorization: String,
    backend: Arc<dyn MatterBackend>,
    jobs: Arc<dyn CommissioningJobStore>,
}

pub trait CommissioningJobStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<CommissioningJob>, String>;
    fn save(&self, job: CommissioningJob) -> Result<(), String>;
}

#[derive(Default)]
pub struct MemoryJobStore(Mutex<HashMap<String, CommissioningJob>>);
impl CommissioningJobStore for MemoryJobStore {
    fn get(&self, id: &str) -> Result<Option<CommissioningJob>, String> {
        self.0
            .lock()
            .map_err(|_| String::from("Matter job store is unavailable"))
            .map(|jobs| jobs.get(id).cloned())
    }
    fn save(&self, job: CommissioningJob) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| String::from("Matter job store is unavailable"))?
            .insert(job.id.clone(), job);
        Ok(())
    }
}

/// Magasin de jobs non secrets, écrit par remplacement atomique. Les données
/// de fabric restent exclusivement dans le backend Matter, jamais ici.
pub struct FileJobStore {
    path: std::path::PathBuf,
    lock: Mutex<()>,
}
impl FileJobStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Result<Self, DaemonError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(DaemonError::Socket)?;
        }
        Ok(Self {
            path,
            lock: Mutex::new(()),
        })
    }
    fn read(&self) -> Result<HashMap<String, CommissioningJob>, String> {
        match fs::read(&self.path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "Matter job store is invalid".into())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(_) => Err("Matter job store is unavailable".into()),
        }
    }
    fn write(&self, jobs: &HashMap<String, CommissioningJob>) -> Result<(), String> {
        let bytes = serde_json::to_vec(jobs)
            .map_err(|_| "Matter job store cannot be serialized".to_string())?;
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temporary, bytes).map_err(|_| "Matter job store is unavailable".to_string())?;
        fs::rename(&temporary, &self.path)
            .map_err(|_| "Matter job store is unavailable".to_string())
    }
}
impl CommissioningJobStore for FileJobStore {
    fn get(&self, id: &str) -> Result<Option<CommissioningJob>, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Matter job store is unavailable".to_string())?;
        Ok(self.read()?.get(id).cloned())
    }
    fn save(&self, job: CommissioningJob) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Matter job store is unavailable".to_string())?;
        let mut jobs = self.read()?;
        jobs.insert(job.id.clone(), job);
        self.write(&jobs)
    }
}

impl MatterDaemon {
    pub fn new(
        authorization: String,
        backend: Arc<dyn MatterBackend>,
    ) -> Result<Self, DaemonError> {
        Self::with_job_store(authorization, backend, Arc::new(MemoryJobStore::default()))
    }

    pub fn with_job_store(
        authorization: String,
        backend: Arc<dyn MatterBackend>,
        jobs: Arc<dyn CommissioningJobStore>,
    ) -> Result<Self, DaemonError> {
        if authorization.trim().is_empty() || authorization.len() > 4096 {
            return Err(DaemonError::InvalidAuthorization);
        }
        Ok(Self {
            authorization,
            backend,
            jobs,
        })
    }

    pub fn handle(
        &self,
        request: AuthenticatedRpcRequest<MatterRequest>,
    ) -> RpcResponse<MatterResponse> {
        let request_id = request.request.request_id.clone();
        let body = if request.validate().is_err() {
            MatterResponse::Error {
                code: "invalid_request".into(),
                detail: "invalid local RPC request".into(),
            }
        } else if !constant_time_eq(&self.authorization, &request.authorization) {
            MatterResponse::Error {
                code: "unauthorized".into(),
                detail: "local RPC authorization failed".into(),
            }
        } else {
            self.dispatch(request.request.body)
        };
        RpcResponse {
            rpc_version: RPC_VERSION,
            request_id,
            body,
        }
    }

    fn dispatch(&self, request: MatterRequest) -> MatterResponse {
        match request {
            MatterRequest::Health => match self.backend.health() {
                Ok((available, detail)) => MatterResponse::Health { available, detail },
                Err(_) => MatterResponse::Health {
                    available: false,
                    detail: Some("Matter backend health check failed".into()),
                },
            },
            MatterRequest::ListEndpoints => match self.backend.list_endpoints() {
                Ok(endpoints) => MatterResponse::Endpoints { endpoints },
                Err(_) => MatterResponse::Error {
                    code: "backend_unavailable".into(),
                    detail: "Matter endpoint discovery failed".into(),
                },
            },
            MatterRequest::Invoke {
                fabric_id,
                node_id,
                endpoint_id,
                command,
            } => match self
                .backend
                .invoke(&fabric_id, &node_id, endpoint_id, command)
            {
                Ok(()) => MatterResponse::InvocationAccepted {
                    invocation_id: uuid::Uuid::new_v4().to_string(),
                },
                Err(_) => MatterResponse::Error {
                    code: "backend_unavailable".into(),
                    detail: "Matter command was not accepted".into(),
                },
            },
            MatterRequest::StartCommissioning { setup_code } => {
                self.start_commissioning(setup_code)
            }
            MatterRequest::GetJob { job_id } => self
                .jobs
                .get(&job_id)
                .ok()
                .flatten()
                .map(|job| MatterResponse::Job { job })
                .unwrap_or_else(|| MatterResponse::Error {
                    code: "job_not_found".into(),
                    detail: "Matter commissioning job was not found".into(),
                }),
        }
    }

    fn start_commissioning(&self, setup_code: String) -> MatterResponse {
        if setup_code.trim().is_empty() || setup_code.len() > 256 {
            return MatterResponse::Error {
                code: "invalid_setup_code".into(),
                detail: "Matter setup code is invalid".into(),
            };
        }
        let id = uuid::Uuid::new_v4().to_string();
        let outcome = self.backend.commission(&setup_code);
        let job = CommissioningJob {
            id: id.clone(),
            status: if outcome.is_ok() {
                JobStatus::Succeeded
            } else {
                JobStatus::Failed
            },
            progress: if outcome.is_ok() { 100 } else { 0 },
            detail: outcome
                .err()
                .map(|_| "Matter commissioning could not start".into()),
        };
        match self.jobs.save(job) {
            Ok(()) => MatterResponse::CommissioningStarted { job_id: id },
            Err(_) => MatterResponse::Error {
                code: "internal".into(),
                detail: "Matter job store is unavailable".into(),
            },
        }
    }
}

pub fn bind_private_socket(path: &Path) -> Result<UnixListener, DaemonError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(DaemonError::Socket)?;
        if !metadata.file_type().is_socket() {
            return Err(DaemonError::SocketPathOccupied);
        }
        std::fs::remove_file(path).map_err(DaemonError::Socket)?;
    }
    let listener = UnixListener::bind(path).map_err(DaemonError::Socket)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(DaemonError::Socket)?;
    Ok(listener)
}

pub fn serve(listener: UnixListener, daemon: Arc<MatterDaemon>) -> Result<(), DaemonError> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let daemon = daemon.clone();
                std::thread::spawn(move || {
                    let _ = serve_connection(stream, &daemon);
                });
            }
            Err(error) => return Err(DaemonError::Socket(error)),
        }
    }
    Ok(())
}

fn serve_connection(mut stream: UnixStream, daemon: &MatterDaemon) -> Result<(), DaemonError> {
    let mut line = String::new();
    BufReader::new(stream.try_clone().map_err(DaemonError::Socket)?)
        .read_line(&mut line)
        .map_err(DaemonError::Socket)?;
    let response = if line.len() > 1_048_576 {
        RpcResponse {
            rpc_version: RPC_VERSION,
            request_id: String::new(),
            body: MatterResponse::Error {
                code: "request_too_large".into(),
                detail: "local RPC request exceeds 1 MiB".into(),
            },
        }
    } else {
        match serde_json::from_str::<AuthenticatedRpcRequest<MatterRequest>>(&line) {
            Ok(request) => daemon.handle(request),
            Err(_) => RpcResponse {
                rpc_version: RPC_VERSION,
                request_id: String::new(),
                body: MatterResponse::Error {
                    code: "invalid_json".into(),
                    detail: "invalid local RPC JSON".into(),
                },
            },
        }
    };
    serde_json::to_writer(&mut stream, &response).map_err(DaemonError::Json)?;
    stream.write_all(b"\n").map_err(DaemonError::Socket)?;
    stream.flush().map_err(DaemonError::Socket)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let mut difference = left.len() ^ right.len();
    for (left, right) in left.bytes().zip(right.bytes()) {
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("Matter local RPC authorization is invalid")]
    InvalidAuthorization,
    #[error("Matter socket path is occupied by a non-socket file")]
    SocketPathOccupied,
    #[error("Matter socket operation failed")]
    Socket(#[source] std::io::Error),
    #[error("Matter RPC serialization failed")]
    Json(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_matter_contract::{Cluster, Endpoint, RpcRequest};

    struct Fake;
    impl MatterBackend for Fake {
        fn health(&self) -> Result<(bool, Option<String>), String> {
            Ok((true, None))
        }
        fn list_endpoints(&self) -> Result<Vec<Endpoint>, String> {
            Ok(vec![Endpoint {
                fabric_id: "f".into(),
                node_id: "n".into(),
                endpoint_id: 1,
                name: "Lampe".into(),
                clusters: vec![Cluster::OnOff],
            }])
        }
        fn invoke(
            &self,
            _: &str,
            _: &str,
            _: u16,
            _: robine_matter_contract::ClusterCommand,
        ) -> Result<(), String> {
            Ok(())
        }
        fn commission(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    fn request(body: MatterRequest, authorization: &str) -> AuthenticatedRpcRequest<MatterRequest> {
        AuthenticatedRpcRequest {
            authorization: authorization.into(),
            request: RpcRequest {
                rpc_version: RPC_VERSION,
                request_id: "req-1".into(),
                body,
            },
        }
    }
    #[test]
    fn rejects_an_unauthorized_request_before_its_backend() {
        let daemon = MatterDaemon::new("secret".into(), Arc::new(Fake)).unwrap();
        assert!(
            matches!(daemon.handle(request(MatterRequest::ListEndpoints, "wrong")).body, MatterResponse::Error { code, .. } if code == "unauthorized")
        );
    }
    #[test]
    fn commissioning_creates_a_queryable_job_without_echoing_the_setup_code() {
        let daemon = MatterDaemon::new("secret".into(), Arc::new(Fake)).unwrap();
        let response = daemon.handle(request(
            MatterRequest::StartCommissioning {
                setup_code: "34970112332".into(),
            },
            "secret",
        ));
        let MatterResponse::CommissioningStarted { job_id } = response.body else {
            panic!("commissioning was not accepted")
        };
        let job = daemon.handle(request(MatterRequest::GetJob { job_id }, "secret"));
        let serialized = serde_json::to_string(&job).unwrap();
        assert!(
            matches!(job.body, MatterResponse::Job { job } if job.status == JobStatus::Succeeded)
        );
        assert!(!serialized.contains("34970112332"));
    }

    #[test]
    fn file_job_store_survives_a_daemon_restart() {
        let path =
            std::env::temp_dir().join(format!("robine-matter-jobs-{}.json", uuid::Uuid::new_v4()));
        let store = Arc::new(FileJobStore::new(&path).unwrap());
        let daemon = MatterDaemon::with_job_store("secret".into(), Arc::new(Fake), store).unwrap();
        let MatterResponse::CommissioningStarted { job_id } = daemon
            .handle(request(
                MatterRequest::StartCommissioning {
                    setup_code: "34970112332".into(),
                },
                "secret",
            ))
            .body
        else {
            panic!("commissioning was not accepted")
        };
        let restarted = MatterDaemon::with_job_store(
            "secret".into(),
            Arc::new(Fake),
            Arc::new(FileJobStore::new(&path).unwrap()),
        )
        .unwrap();
        assert!(matches!(
            restarted
                .handle(request(MatterRequest::GetJob { job_id }, "secret"))
                .body,
            MatterResponse::Job { job } if job.status == JobStatus::Succeeded
        ));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_a_regular_file_when_binding_the_socket() {
        let path =
            std::env::temp_dir().join(format!("robine-matter-socket-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"do not replace").unwrap();
        assert!(matches!(
            bind_private_socket(&path),
            Err(DaemonError::SocketPathOccupied)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"do not replace");
        std::fs::remove_file(path).unwrap();
    }
}
