//! Sauvegarde/restauration SQLite cohérente, sans inclure de secrets.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, backup::Backup};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;

pub const BACKUP_MANIFEST_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub manifest_version: u16,
    pub created_at: DateTime<Utc>,
    pub database_file: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn create_snapshot(
    database: &Path,
    backup_directory: &Path,
) -> Result<BackupManifest, BackupError> {
    fs::create_dir_all(backup_directory).map_err(BackupError::Io)?;
    let created_at = Utc::now();
    let file_stem = format!(
        "robine-{}-{}",
        created_at.format("%Y%m%dT%H%M%SZ"),
        uuid::Uuid::new_v4()
    );
    let database_file = format!("{file_stem}.sqlite3");
    let destination = backup_directory.join(&database_file);
    sqlite_copy(database, &destination)?;
    let manifest = BackupManifest {
        manifest_version: BACKUP_MANIFEST_VERSION,
        created_at,
        database_file,
        bytes: fs::metadata(&destination).map_err(BackupError::Io)?.len(),
        sha256: sha256_file(&destination)?,
    };
    let manifest_path = backup_directory.join(format!("{file_stem}.manifest.json"));
    fs::write(
        manifest_path,
        serde_json::to_vec_pretty(&manifest).map_err(BackupError::Manifest)?,
    )
    .map_err(BackupError::Io)?;
    Ok(manifest)
}

pub fn verify_snapshot(
    backup_directory: &Path,
    manifest: &BackupManifest,
) -> Result<PathBuf, BackupError> {
    if manifest.manifest_version != BACKUP_MANIFEST_VERSION {
        return Err(BackupError::UnsupportedManifest(manifest.manifest_version));
    }
    let database = backup_directory.join(&manifest.database_file);
    let metadata = fs::metadata(&database).map_err(BackupError::Io)?;
    if metadata.len() != manifest.bytes {
        return Err(BackupError::SizeMismatch);
    }
    if sha256_file(&database)? != manifest.sha256 {
        return Err(BackupError::ChecksumMismatch);
    }
    let connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(BackupError::Sqlite)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(BackupError::Sqlite)?;
    if integrity != "ok" {
        return Err(BackupError::Integrity(integrity));
    }
    Ok(database)
}

/// Restaure une sauvegarde seulement après sa vérification. La base active est
/// déplacée vers un fichier préventif ; le caller peut le conserver ou le purger
/// dans son propre parcours d'administration.
pub fn restore_snapshot(
    database: &Path,
    backup_directory: &Path,
    manifest: &BackupManifest,
) -> Result<PathBuf, BackupError> {
    let source = verify_snapshot(backup_directory, manifest)?;
    let parent = database.parent().ok_or(BackupError::DatabasePath)?;
    let staged = parent.join(format!(".robine-restore-{}.sqlite3", uuid::Uuid::new_v4()));
    sqlite_copy(&source, &staged)?;
    let staged_connection = Connection::open_with_flags(
        &staged,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(BackupError::Sqlite)?;
    let integrity: String = staged_connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(BackupError::Sqlite)?;
    drop(staged_connection);
    if integrity != "ok" {
        let _ = fs::remove_file(&staged);
        return Err(BackupError::Integrity(integrity));
    }
    let previous = parent.join(format!(
        "robine-pre-restore-{}.sqlite3",
        Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    if database.exists() {
        fs::rename(database, &previous).map_err(BackupError::Io)?;
    }
    if let Err(error) = fs::rename(&staged, database) {
        if previous.exists() {
            let _ = fs::rename(&previous, database);
        }
        return Err(BackupError::Io(error));
    }
    Ok(previous)
}

fn sqlite_copy(source_path: &Path, destination_path: &Path) -> Result<(), BackupError> {
    let source = Connection::open_with_flags(
        source_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(BackupError::Sqlite)?;
    let mut destination = Connection::open(destination_path).map_err(BackupError::Sqlite)?;
    let backup = Backup::new(&source, &mut destination).map_err(BackupError::Sqlite)?;
    backup
        .run_to_completion(128, Duration::from_millis(5), None)
        .map_err(BackupError::Sqlite)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, BackupError> {
    let mut file = fs::File::open(path).map_err(BackupError::Io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(BackupError::Io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("backup I/O failed: {0}")]
    Io(#[source] std::io::Error),
    #[error("SQLite backup failed: {0}")]
    Sqlite(#[source] rusqlite::Error),
    #[error("backup manifest failed: {0}")]
    Manifest(#[source] serde_json::Error),
    #[error("unsupported backup manifest version {0}")]
    UnsupportedManifest(u16),
    #[error("backup size does not match its manifest")]
    SizeMismatch,
    #[error("backup checksum does not match its manifest")]
    ChecksumMismatch,
    #[error("SQLite integrity check failed: {0}")]
    Integrity(String),
    #[error("database path has no parent directory")]
    DatabasePath,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_is_verified_before_restore() {
        let root =
            std::env::temp_dir().join(format!("robine-backup-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("robine.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE note (text TEXT); INSERT INTO note VALUES ('coeur');")
            .unwrap();
        drop(connection);
        let backup_directory = root.join("backups");
        let manifest = create_snapshot(&database, &backup_directory).unwrap();
        verify_snapshot(&backup_directory, &manifest).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
