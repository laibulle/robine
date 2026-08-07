use robine_matterd::{FileJobStore, MatterDaemon, UnavailableBackend, bind_private_socket, serve};
use robine_secret_store::{MacOsKeychainSecretStore, SecretStore};
use std::{env, path::PathBuf, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = env::var_os("ROBINE_MATTER_SOCKET")
        .map(PathBuf::from)
        .ok_or("ROBINE_MATTER_SOCKET is required")?;
    let secrets = MacOsKeychainSecretStore::new("io.robine.server")?;
    let authorization = secrets
        .get("matter:local-rpc")?
        .ok_or("Matter local RPC secret is unavailable")?;
    let jobs = Arc::new(FileJobStore::new(socket.with_extension("jobs.json"))?);
    let daemon = Arc::new(MatterDaemon::with_job_store(
        authorization,
        Arc::new(UnavailableBackend),
        jobs,
    )?);
    serve(bind_private_socket(&socket)?, daemon)?;
    Ok(())
}
