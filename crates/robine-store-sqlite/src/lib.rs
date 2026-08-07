//! Adaptateur SQLite de Robine. Toutes les mutations sont sérialisées par cette
//! instance; l'API l'appelle depuis un worker bloquant Actix.

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use rand::RngCore;
use robine_application::{
    ApplicationError, DevicePage, DevicePageRequest, EventStream, FlowAdmission, HomeRepository,
};
use robine_domain::*;
use robine_flow_plan::ConcurrencyMode;
use robine_mcp_types::{McpWritePolicy, Scope, Scopes};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpTokenIdentity {
    pub token_id: String,
    pub scopes: Vec<String>,
    pub write_policy: McpWritePolicy,
}

/// Configuration non secrète d'un bridge Hue. La clé d'application est gardée
/// exclusivement dans le trousseau système et référencée par `secret_name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HueBridgeConfiguration {
    pub authority: String,
    pub certificate_pem: String,
    pub certificate_sha256: String,
    pub secret_name: String,
}

/// Résultat d'un passage de compaction. Les suppressions sont bornées par
/// table afin de ne pas immobiliser le writer SQLite sur une grande base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionPurge {
    pub events: usize,
    pub flow_run_traces: usize,
    pub flow_trigger_claims: usize,
}

impl RetentionPurge {
    pub fn is_empty(self) -> bool {
        self.events == 0 && self.flow_run_traces == 0 && self.flow_trigger_claims == 0
    }
}

pub struct SqliteStore {
    connection: Mutex<Connection>,
    events: broadcast::Sender<EventEnvelope>,
}

const BASE_SCHEMA_VERSION: i64 = 1;
const BASE_SCHEMA_CHECKSUM: &str = "robine-sqlite-schema-v1";
const FLOW_TRIGGER_DEDUP_SCHEMA_VERSION: i64 = 2;
const FLOW_TRIGGER_DEDUP_SCHEMA_CHECKSUM: &str = "robine-flow-trigger-dedup-v2";
const MCP_APPROVAL_SCHEMA_VERSION: i64 = 3;
const MCP_APPROVAL_SCHEMA_CHECKSUM: &str = "robine-mcp-approvals-v3";
const FLOW_AWAIT_SCHEMA_VERSION: i64 = 4;
const FLOW_AWAIT_SCHEMA_CHECKSUM: &str = "robine-flow-await-v4";
const DEVICE_PAGE_SCHEMA_VERSION: i64 = 5;
const DEVICE_PAGE_SCHEMA_CHECKSUM: &str = "robine-device-page-v5";
const MCP_ALLOW_LIST_SCHEMA_VERSION: i64 = 6;
const MCP_ALLOW_LIST_SCHEMA_CHECKSUM: &str = "robine-mcp-allow-list-v6";
const FLOW_TRACE_SCHEMA_VERSION: i64 = 7;
const FLOW_TRACE_SCHEMA_CHECKSUM: &str = "robine-flow-traces-v7";
const FLOW_CONCURRENCY_SCHEMA_VERSION: i64 = 8;
const FLOW_CONCURRENCY_SCHEMA_CHECKSUM: &str = "robine-flow-concurrency-v8";
const AUTOMATION_ENGINE_CURSOR_SCHEMA_VERSION: i64 = 9;
const AUTOMATION_ENGINE_CURSOR_SCHEMA_CHECKSUM: &str = "robine-automation-engine-cursor-v9";
const RETENTION_INDEX_SCHEMA_VERSION: i64 = 10;
const RETENTION_INDEX_SCHEMA_CHECKSUM: &str = "robine-retention-indexes-v10";
const DEFAULT_RETENTION_DAYS: i64 = 30;

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        let connection = Connection::open(path).map_err(sql_error)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, adapter_id TEXT NOT NULL, protocol_address TEXT NOT NULL, sort_name TEXT NOT NULL, status TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(adapter_id, protocol_address));
             CREATE TABLE IF NOT EXISTS entities (id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id), protocol_address TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(device_id, protocol_address));
             CREATE TABLE IF NOT EXISTS areas (id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE);
             CREATE TABLE IF NOT EXISTS adapter_health (adapter_id TEXT PRIMARY KEY, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS flows (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS flow_runs (id TEXT PRIMARY KEY, wake_at TEXT NOT NULL, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS flow_trigger_claims (flow_id TEXT NOT NULL, correlation_id TEXT NOT NULL, claimed_at TEXT NOT NULL, PRIMARY KEY(flow_id, correlation_id));
             CREATE TABLE IF NOT EXISTS entity_state (entity_id TEXT NOT NULL REFERENCES entities(id), property_key TEXT NOT NULL, source_at TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(entity_id, property_key));
             CREATE TABLE IF NOT EXISTS events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, occurred_at TEXT NOT NULL, correlation_id TEXT, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS commands (id TEXT PRIMARY KEY, idempotency_key TEXT NOT NULL UNIQUE, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS administrators (id INTEGER PRIMARY KEY CHECK(id = 1), password_hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS api_tokens (token_hash TEXT PRIMARY KEY, created_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS mcp_tokens (token_hash TEXT PRIMARY KEY, expires_at TEXT NOT NULL, scopes TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS hue_bridges (authority TEXT PRIMARY KEY, certificate_pem TEXT NOT NULL, certificate_sha256 TEXT NOT NULL, secret_name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, checksum TEXT NOT NULL);",
        ).map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![BASE_SCHEMA_VERSION, BASE_SCHEMA_CHECKSUM],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![BASE_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != BASE_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![
                    FLOW_TRIGGER_DEDUP_SCHEMA_VERSION,
                    FLOW_TRIGGER_DEDUP_SCHEMA_CHECKSUM
                ],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![FLOW_TRIGGER_DEDUP_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != FLOW_TRIGGER_DEDUP_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection
            .execute_batch(
                "ALTER TABLE mcp_tokens ADD COLUMN token_id TEXT;
             CREATE UNIQUE INDEX IF NOT EXISTS mcp_tokens_token_id ON mcp_tokens(token_id);
             CREATE TABLE IF NOT EXISTS mcp_approvals (
               approval_hash TEXT PRIMARY KEY,
               token_id TEXT NOT NULL,
               tool TEXT NOT NULL,
               arguments_hash TEXT NOT NULL,
               expires_at TEXT NOT NULL,
               consumed_at TEXT
             );
             CREATE TABLE IF NOT EXISTS mcp_audit (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,
               occurred_at TEXT NOT NULL,
               token_id TEXT NOT NULL,
               tool TEXT NOT NULL,
               arguments_hash TEXT NOT NULL,
               approval_hash TEXT NOT NULL,
               outcome TEXT NOT NULL
             );",
            )
            .or_else(|error| {
                // Une base créée par une version qui a déjà exécuté l'ALTER peut
                // seulement être reprise si le reste du schéma est bien présent.
                if error.to_string().contains("duplicate column name") {
                    connection.execute_batch(
                    "CREATE UNIQUE INDEX IF NOT EXISTS mcp_tokens_token_id ON mcp_tokens(token_id);
                     CREATE TABLE IF NOT EXISTS mcp_approvals (
                       approval_hash TEXT PRIMARY KEY, token_id TEXT NOT NULL, tool TEXT NOT NULL,
                       arguments_hash TEXT NOT NULL, expires_at TEXT NOT NULL, consumed_at TEXT
                     );
                     CREATE TABLE IF NOT EXISTS mcp_audit (
                       sequence INTEGER PRIMARY KEY AUTOINCREMENT, occurred_at TEXT NOT NULL,
                       token_id TEXT NOT NULL, tool TEXT NOT NULL, arguments_hash TEXT NOT NULL,
                       approval_hash TEXT NOT NULL, outcome TEXT NOT NULL
                     );",
                )
                } else {
                    Err(error)
                }
            })
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![MCP_APPROVAL_SCHEMA_VERSION, MCP_APPROVAL_SCHEMA_CHECKSUM],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![MCP_APPROVAL_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != MCP_APPROVAL_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection
            .execute_batch(
                "ALTER TABLE flow_runs ADD COLUMN awaiting INTEGER NOT NULL DEFAULT 0;
                 CREATE INDEX IF NOT EXISTS flow_runs_awaiting ON flow_runs(awaiting, wake_at);",
            )
            .or_else(|error| {
                if error.to_string().contains("duplicate column name") {
                    connection.execute_batch(
                        "CREATE INDEX IF NOT EXISTS flow_runs_awaiting ON flow_runs(awaiting, wake_at);",
                    )
                } else {
                    Err(error)
                }
            })
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![FLOW_AWAIT_SCHEMA_VERSION, FLOW_AWAIT_SCHEMA_CHECKSUM],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![FLOW_AWAIT_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != FLOW_AWAIT_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        ensure_column(
            &connection,
            "devices",
            "sort_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "devices",
            "status",
            "TEXT NOT NULL DEFAULT 'available'",
        )?;
        connection
            .execute_batch(
                "UPDATE devices
                 SET sort_name = json_extract(payload, '$.name'),
                     status = json_extract(payload, '$.status')
                 WHERE sort_name = '' OR status = '';
                 CREATE INDEX IF NOT EXISTS devices_page_by_name
                    ON devices(sort_name COLLATE NOCASE, id);
                 CREATE INDEX IF NOT EXISTS devices_page_by_status_and_name
                    ON devices(status, sort_name COLLATE NOCASE, id);",
            )
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![DEVICE_PAGE_SCHEMA_VERSION, DEVICE_PAGE_SCHEMA_CHECKSUM],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![DEVICE_PAGE_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != DEVICE_PAGE_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        ensure_column(
            &connection,
            "mcp_tokens",
            "write_policy",
            "TEXT NOT NULL DEFAULT '{\"mode\":\"read_only\"}'",
        )?;
        connection
            .execute_batch(
                "UPDATE mcp_tokens
                 SET write_policy = '{\"mode\":\"confirm_each\"}'
                 WHERE write_policy = '{\"mode\":\"read_only\"}'
                   AND (scopes LIKE '%control%' OR scopes LIKE '%automation_write%');
                 CREATE TABLE IF NOT EXISTS mcp_allow_list_usage (
                   token_id TEXT NOT NULL,
                   window_epoch INTEGER NOT NULL,
                   command_count INTEGER NOT NULL,
                   PRIMARY KEY(token_id, window_epoch)
                 );",
            )
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![
                    MCP_ALLOW_LIST_SCHEMA_VERSION,
                    MCP_ALLOW_LIST_SCHEMA_CHECKSUM
                ],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![MCP_ALLOW_LIST_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != MCP_ALLOW_LIST_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS flow_run_traces (
               id TEXT PRIMARY KEY,
               flow_id TEXT NOT NULL,
               recorded_at TEXT NOT NULL,
               payload TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS flow_run_traces_by_flow ON flow_run_traces(flow_id, recorded_at DESC);",
        ).map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![FLOW_TRACE_SCHEMA_VERSION, FLOW_TRACE_SCHEMA_CHECKSUM],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![FLOW_TRACE_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != FLOW_TRACE_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        ensure_column(
            &connection,
            "flow_runs",
            "flow_id",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &connection,
            "flow_runs",
            "queued",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        connection
            .execute_batch(
                "UPDATE flow_runs
                 SET flow_id = json_extract(payload, '$.flow_id')
                 WHERE flow_id = '';
                 CREATE INDEX IF NOT EXISTS flow_runs_by_flow_queue
                    ON flow_runs(flow_id, queued, wake_at, id);",
            )
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![
                    FLOW_CONCURRENCY_SCHEMA_VERSION,
                    FLOW_CONCURRENCY_SCHEMA_CHECKSUM
                ],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![FLOW_CONCURRENCY_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != FLOW_CONCURRENCY_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS automation_engine_cursor (
               id INTEGER PRIMARY KEY CHECK(id = 1),
               sequence INTEGER NOT NULL
             );",
            )
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![
                    AUTOMATION_ENGINE_CURSOR_SCHEMA_VERSION,
                    AUTOMATION_ENGINE_CURSOR_SCHEMA_CHECKSUM
                ],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![AUTOMATION_ENGINE_CURSOR_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != AUTOMATION_ENGINE_CURSOR_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        connection
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS events_by_occurred_at ON events(occurred_at, sequence);
                 CREATE INDEX IF NOT EXISTS flow_run_traces_by_recorded_at ON flow_run_traces(recorded_at, id);
                 CREATE INDEX IF NOT EXISTS flow_trigger_claims_by_claimed_at ON flow_trigger_claims(claimed_at);",
            )
            .map_err(sql_error)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO schema_migrations (version, checksum) VALUES (?1, ?2)",
                params![
                    RETENTION_INDEX_SCHEMA_VERSION,
                    RETENTION_INDEX_SCHEMA_CHECKSUM
                ],
            )
            .map_err(sql_error)?;
        let checksum: String = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![RETENTION_INDEX_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if checksum != RETENTION_INDEX_SCHEMA_CHECKSUM {
            return Err(ApplicationError::Infrastructure(
                "SQLite migration history checksum does not match".into(),
            ));
        }
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(sql_error)?;
        if integrity != "ok" {
            return Err(ApplicationError::Infrastructure(format!(
                "SQLite integrity check failed: {integrity}"
            )));
        }
        let (events, _) = broadcast::channel(512);
        Ok(Self {
            connection: Mutex::new(connection),
            events,
        })
    }

    pub fn open_in_memory() -> Result<Self, ApplicationError> {
        Self::open(":memory:")
    }

    /// Supprime au plus `batch_size` lignes de chaque historique expiré.
    /// La séquence des événements n'est jamais réutilisée : un curseur de
    /// client ou du moteur peut donc rester supérieur au plus vieux événement
    /// conservé et déclencher une resynchronisation normale.
    pub fn prune_retained_data(
        &self,
        now: DateTime<Utc>,
        batch_size: usize,
    ) -> Result<RetentionPurge, ApplicationError> {
        if !(1..=10_000).contains(&batch_size) {
            return Err(ApplicationError::Validation(
                "Retention batch size must be between 1 and 10000".into(),
            ));
        }
        let cutoff = (now - chrono::Duration::days(DEFAULT_RETENTION_DAYS)).to_rfc3339();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let events = transaction
            .execute(
                "DELETE FROM events WHERE sequence IN (
                   SELECT sequence FROM events WHERE occurred_at < ?1 ORDER BY sequence LIMIT ?2
                 )",
                params![&cutoff, batch_size as i64],
            )
            .map_err(sql_error)?;
        let flow_run_traces = transaction
            .execute(
                "DELETE FROM flow_run_traces WHERE id IN (
                   SELECT id FROM flow_run_traces WHERE recorded_at < ?1 ORDER BY recorded_at, id LIMIT ?2
                 )",
                params![&cutoff, batch_size as i64],
            )
            .map_err(sql_error)?;
        let flow_trigger_claims = transaction
            .execute(
                "DELETE FROM flow_trigger_claims WHERE rowid IN (
                   SELECT rowid FROM flow_trigger_claims WHERE claimed_at < ?1 ORDER BY claimed_at LIMIT ?2
                 )",
                params![&cutoff, batch_size as i64],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(RetentionPurge {
            events,
            flow_run_traces,
            flow_trigger_claims,
        })
    }

    pub fn is_initialized(&self) -> Result<bool, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let exists: i64 = connection
            .query_row("SELECT EXISTS(SELECT 1 FROM administrators)", [], |row| {
                row.get(0)
            })
            .map_err(sql_error)?;
        Ok(exists != 0)
    }

    /// Curseur durable du consommateur interne des événements Flow. Ce n'est
    /// pas un curseur client : il permet au runtime de rejouer le journal après
    /// redémarrage ou saturation du broadcast.
    pub fn automation_engine_cursor(&self) -> Result<Option<u64>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let sequence: Option<i64> = connection
            .query_row(
                "SELECT sequence FROM automation_engine_cursor WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        Ok(sequence.map(|sequence| sequence.max(0) as u64))
    }

    pub fn save_automation_engine_cursor(&self, sequence: u64) -> Result<(), ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .execute(
                "INSERT INTO automation_engine_cursor (id, sequence) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET sequence = MAX(sequence, excluded.sequence)",
                params![sequence as i64],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    /// Returns the one-time plaintext bearer token. Only its SHA-256 verifier is persisted.
    pub fn bootstrap_administrator(
        &self,
        password: &str,
        now: DateTime<Utc>,
    ) -> Result<String, ApplicationError> {
        if password.len() < 12 {
            return Err(ApplicationError::Validation(
                "password must contain at least 12 characters".into(),
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let exists: i64 = transaction
            .query_row("SELECT EXISTS(SELECT 1 FROM administrators)", [], |row| {
                row.get(0)
            })
            .map_err(sql_error)?;
        if exists != 0 {
            return Err(ApplicationError::Validation(
                "administrator already initialized".into(),
            ));
        }
        let mut salt_bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut salt_bytes);
        let salt = argon2::password_hash::SaltString::encode_b64(&salt_bytes)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?
            .to_string();
        transaction
            .execute(
                "INSERT INTO administrators (id, password_hash) VALUES (1, ?1)",
                params![password_hash],
            )
            .map_err(sql_error)?;
        let token = new_token();
        transaction
            .execute(
                "INSERT INTO api_tokens (token_hash, created_at) VALUES (?1, ?2)",
                params![token_hash(&token), now.to_rfc3339()],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(token)
    }

    pub fn authenticate(&self, bearer: &str) -> Result<bool, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let found: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM api_tokens WHERE token_hash = ?1",
                params![token_hash(bearer)],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        Ok(found.is_some())
    }

    pub fn save_hue_bridge(
        &self,
        configuration: &HueBridgeConfiguration,
    ) -> Result<(), ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection.execute(
            "INSERT INTO hue_bridges (authority, certificate_pem, certificate_sha256, secret_name) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(authority) DO UPDATE SET certificate_pem = excluded.certificate_pem, certificate_sha256 = excluded.certificate_sha256, secret_name = excluded.secret_name",
            params![configuration.authority, configuration.certificate_pem, configuration.certificate_sha256, configuration.secret_name],
        ).map_err(sql_error)?;
        Ok(())
    }

    pub fn list_hue_bridges(&self) -> Result<Vec<HueBridgeConfiguration>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection.prepare("SELECT authority, certificate_pem, certificate_sha256, secret_name FROM hue_bridges ORDER BY authority").map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok(HueBridgeConfiguration {
                    authority: row.get(0)?,
                    certificate_pem: row.get(1)?,
                    certificate_sha256: row.get(2)?,
                    secret_name: row.get(3)?,
                })
            })
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)
    }

    pub fn issue_token(
        &self,
        password: &str,
        now: DateTime<Utc>,
    ) -> Result<String, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let hash: String = connection
            .query_row(
                "SELECT password_hash FROM administrators WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?
            .ok_or_else(|| ApplicationError::Validation("administrator not initialized".into()))?;
        let parsed = PasswordHash::new(&hash)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| ApplicationError::Validation("invalid administrator password".into()))?;
        let token = new_token();
        connection
            .execute(
                "INSERT INTO api_tokens (token_hash, created_at) VALUES (?1, ?2)",
                params![token_hash(&token), now.to_rfc3339()],
            )
            .map_err(sql_error)?;
        Ok(token)
    }

    /// Émet un jeton MCP dédié, sans privilège de contrôle. Les capacités
    /// d'écriture ne sont pas distribuées avant l'implémentation des politiques
    /// d'approbation explicite.
    pub fn issue_read_mcp_token(
        &self,
        expires_in_seconds: u64,
        now: DateTime<Utc>,
    ) -> Result<(String, DateTime<Utc>), ApplicationError> {
        self.issue_mcp_token(&["read".to_owned()], None, expires_in_seconds, now)
            .map(|(token, _, expires_at)| (token, expires_at))
    }

    /// Émet un jeton MCP à capacités explicites. Le jeton reste séparé du
    /// jeton d'administration HTTP et son secret n'est jamais persisté.
    pub fn issue_mcp_token(
        &self,
        requested_scopes: &[String],
        requested_policy: Option<McpWritePolicy>,
        expires_in_seconds: u64,
        now: DateTime<Utc>,
    ) -> Result<(String, McpTokenIdentity, DateTime<Utc>), ApplicationError> {
        if !(60..=2_592_000).contains(&expires_in_seconds) {
            return Err(ApplicationError::Validation(
                "MCP token expiry must be between one minute and thirty days".into(),
            ));
        }
        let scopes = normalize_mcp_scopes(requested_scopes)?;
        let typed_scopes =
            Scopes::new(scopes.iter().filter_map(|scope| Scope::from_storage(scope)));
        let write_policy =
            requested_policy.unwrap_or_else(|| McpWritePolicy::default_for(&typed_scopes));
        write_policy
            .validate_for(&typed_scopes)
            .map_err(|message| ApplicationError::Validation(message.into()))?;
        if let McpWritePolicy::AllowListed { commands, .. } = &write_policy {
            for command in commands {
                uuid::Uuid::parse_str(&command.entity_id).map_err(|_| {
                    ApplicationError::Validation(
                        "allow-listed MCP entity identifiers must be UUIDs".into(),
                    )
                })?;
            }
        }
        let expires_at = now + chrono::Duration::seconds(expires_in_seconds as i64);
        let token = new_token();
        let token_id = uuid::Uuid::new_v4().to_string();
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .execute(
                "INSERT INTO mcp_tokens (token_hash, expires_at, scopes, token_id, write_policy) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![token_hash(&token), expires_at.to_rfc3339(), scopes.join(","), token_id, serde_json::to_string(&write_policy).map_err(json_error)?],
            )
            .map_err(sql_error)?;
        Ok((
            token,
            McpTokenIdentity {
                token_id,
                scopes,
                write_policy,
            },
            expires_at,
        ))
    }

    pub fn authenticate_mcp_read(
        &self,
        bearer: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, ApplicationError> {
        Ok(self
            .authenticate_mcp(bearer, now)?
            .is_some_and(|identity| identity.scopes.iter().any(|scope| scope == "read")))
    }

    pub fn authenticate_mcp(
        &self,
        bearer: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<McpTokenIdentity>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let row: Option<(String, String, Option<String>, String)> = connection
            .query_row(
                "SELECT expires_at, scopes, token_id, write_policy FROM mcp_tokens WHERE token_hash = ?1",
                params![token_hash(bearer)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(sql_error)?;
        let Some((expires_at, scopes, token_id, write_policy)) = row else {
            return Ok(None);
        };
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)
            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?
            .with_timezone(&Utc);
        if expires_at <= now {
            return Ok(None);
        }
        let scopes =
            normalize_mcp_scopes(&scopes.split(',').map(str::to_owned).collect::<Vec<_>>())?;
        let typed_scopes =
            Scopes::new(scopes.iter().filter_map(|scope| Scope::from_storage(scope)));
        let write_policy =
            serde_json::from_str::<McpWritePolicy>(&write_policy).map_err(json_error)?;
        write_policy
            .validate_for(&typed_scopes)
            .map_err(|message| {
                ApplicationError::Infrastructure(format!(
                    "invalid persisted MCP write policy: {message}"
                ))
            })?;
        Ok(Some(McpTokenIdentity {
            token_id: token_id.unwrap_or_else(|| token_hash(bearer)),
            scopes,
            write_policy,
        }))
    }

    pub fn create_mcp_approval(
        &self,
        token_id: &str,
        tool: &str,
        arguments_hash: &str,
        expires_in_seconds: u64,
        now: DateTime<Utc>,
    ) -> Result<(String, DateTime<Utc>), ApplicationError> {
        if !(30..=3_600).contains(&expires_in_seconds) {
            return Err(ApplicationError::Validation(
                "MCP approval expiry must be between thirty seconds and one hour".into(),
            ));
        }
        let required_scope = match tool {
            "robine.command.request" => "control",
            "robine.automation.set-enabled" => "automation_write",
            _ => {
                return Err(ApplicationError::Validation(
                    "MCP tool is not approvable".into(),
                ));
            }
        };
        if arguments_hash.len() != 64
            || !arguments_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ApplicationError::Validation(
                "MCP approval arguments hash is invalid".into(),
            ));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let token: Option<(String, String)> = transaction
            .query_row(
                "SELECT scopes, write_policy FROM mcp_tokens WHERE token_id = ?1 AND expires_at > ?2",
                params![token_id, now.to_rfc3339()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sql_error)?;
        let Some((scopes, write_policy)) = token else {
            return Err(ApplicationError::Validation(
                "MCP token is unknown or expired".into(),
            ));
        };
        let write_policy =
            serde_json::from_str::<McpWritePolicy>(&write_policy).map_err(json_error)?;
        if matches!(write_policy, McpWritePolicy::AllowListed { .. }) {
            return Err(ApplicationError::Validation(
                "allow-listed MCP tokens do not use per-call approvals".into(),
            ));
        }
        if !scopes.split(',').any(|scope| scope == required_scope) {
            return Err(ApplicationError::Validation(
                "MCP token does not have the required scope".into(),
            ));
        }
        let approval_id = uuid::Uuid::new_v4().to_string();
        let expires_at = now + chrono::Duration::seconds(expires_in_seconds as i64);
        transaction.execute(
            "INSERT INTO mcp_approvals (approval_hash, token_id, tool, arguments_hash, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![token_hash(&approval_id), token_id, tool, arguments_hash, expires_at.to_rfc3339()],
        ).map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok((approval_id, expires_at))
    }

    /// Consomme une approbation exactement une fois et écrit l'issue dans
    /// l'audit, y compris lorsque l'approbation est refusée ou déjà utilisée.
    pub fn consume_mcp_approval(
        &self,
        token_id: &str,
        tool: &str,
        arguments_hash: &str,
        approval_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, ApplicationError> {
        let approval_hash = token_hash(approval_id);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let consumed = transaction.execute(
            "UPDATE mcp_approvals SET consumed_at = ?1 WHERE approval_hash = ?2 AND token_id = ?3 AND tool = ?4 AND arguments_hash = ?5 AND consumed_at IS NULL AND expires_at > ?1",
            params![now.to_rfc3339(), approval_hash, token_id, tool, arguments_hash],
        ).map_err(sql_error)? == 1;
        transaction.execute(
            "INSERT INTO mcp_audit (occurred_at, token_id, tool, arguments_hash, approval_hash, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now.to_rfc3339(), token_id, tool, arguments_hash, approval_hash, if consumed { "approved" } else { "denied" }],
        ).map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(consumed)
    }

    /// Réserve atomiquement un créneau de la liste blanche. Les compteurs sont
    /// ramenés à une fenêtre UTC fixe d'une heure et chaque refus est audité,
    /// de sorte qu'un client ne puisse ni contourner le quota ni faire gonfler
    /// la mémoire du processus.
    pub fn claim_mcp_allow_listed_command(
        &self,
        token_id: &str,
        tool: &str,
        arguments_hash: &str,
        max_commands_per_hour: u32,
        now: DateTime<Utc>,
    ) -> Result<bool, ApplicationError> {
        if tool != "robine.command.request" || !(1..=3_600).contains(&max_commands_per_hour) {
            return Err(ApplicationError::Validation(
                "invalid MCP allow-list command claim".into(),
            ));
        }
        let window_epoch = now.timestamp().div_euclid(3_600) * 3_600;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let used: u32 = transaction
            .query_row(
                "SELECT command_count FROM mcp_allow_list_usage WHERE token_id = ?1 AND window_epoch = ?2",
                params![token_id, window_epoch],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?
            .unwrap_or(0);
        let accepted = used < max_commands_per_hour;
        if accepted {
            transaction.execute(
                "INSERT INTO mcp_allow_list_usage (token_id, window_epoch, command_count) VALUES (?1, ?2, 1)
                 ON CONFLICT(token_id, window_epoch) DO UPDATE SET command_count = command_count + 1",
                params![token_id, window_epoch],
            ).map_err(sql_error)?;
        }
        transaction.execute(
            "INSERT INTO mcp_audit (occurred_at, token_id, tool, arguments_hash, approval_hash, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now.to_rfc3339(), token_id, tool, arguments_hash, "allow-listed", if accepted { "allow_listed" } else { "quota_denied" }],
        ).map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(accepted)
    }

    fn device_from_row(
        connection: &Connection,
        id: &str,
    ) -> Result<Option<Device>, ApplicationError> {
        let mut device: Option<Device> = connection
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()?;
        if let Some(device) = &mut device {
            let mut statement = connection
                .prepare("SELECT payload FROM entities WHERE device_id = ?1 ORDER BY rowid")
                .map_err(sql_error)?;
            device.entities = statement
                .query_map(params![id], |row| row.get::<_, String>(0))
                .map_err(sql_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_error)?
                .into_iter()
                .map(|json| serde_json::from_str(&json).map_err(json_error))
                .collect::<Result<_, _>>()?;
        }
        Ok(device)
    }
}

impl HomeRepository for SqliteStore {
    fn register_discovery(
        &self,
        discovery: DeviceDiscovery,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let existing_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM devices WHERE adapter_id = ?1 AND protocol_address = ?2",
                params![&discovery.adapter_id.0, &discovery.protocol_address],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        let (id, was_existing) = match existing_id {
            Some(id) => (
                DeviceId(
                    uuid::Uuid::parse_str(&id)
                        .map_err(|e| ApplicationError::Infrastructure(e.to_string()))?,
                ),
                true,
            ),
            None => (DeviceId::new(), false),
        };
        let previous_device: Option<Device> = transaction
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|payload| serde_json::from_str(&payload).map_err(json_error))
            .transpose()?;
        let mut existing = existing_entities(&transaction, &id)?;
        let mut persisted_entities = Vec::with_capacity(discovery.entities.len());
        for announced in discovery.entities {
            let previous = existing.remove(&announced.protocol_address);
            let entity = Entity {
                id: previous
                    .as_ref()
                    .map(|entity| entity.id.clone())
                    .unwrap_or_else(EntityId::new),
                name: previous
                    .as_ref()
                    .map(|entity| entity.name.clone())
                    .unwrap_or(announced.name),
                kind: announced.kind,
                capabilities: announced.capabilities,
                area_id: previous.and_then(|entity| entity.area_id),
            };
            persisted_entities.push((announced.protocol_address, entity));
        }
        let device = Device {
            id: id.clone(),
            adapter_id: discovery.adapter_id,
            protocol_address: discovery.protocol_address,
            name: previous_device
                .as_ref()
                .map(|device| device.name.clone())
                .unwrap_or(discovery.name),
            status: DeviceStatus::Available,
            entities: persisted_entities
                .iter()
                .map(|(_, entity)| entity.clone())
                .collect(),
        };
        transaction.execute("INSERT INTO devices (id, adapter_id, protocol_address, sort_name, status, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(adapter_id, protocol_address) DO UPDATE SET sort_name = excluded.sort_name, status = excluded.status, payload = excluded.payload", params![id.to_string(), &device.adapter_id.0, &device.protocol_address, &device.name, device_status_storage(&device.status), to_json(&device)?]).map_err(sql_error)?;
        for (protocol_address, entity) in persisted_entities {
            transaction.execute("INSERT INTO entities (id, device_id, protocol_address, payload) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(device_id, protocol_address) DO UPDATE SET payload = excluded.payload", params![entity.id.to_string(), id.to_string(), protocol_address, to_json(&entity)?]).map_err(sql_error)?;
        }
        let data = if was_existing {
            EventData::DeviceUpdated {
                device: device.clone(),
            }
        } else {
            EventData::DeviceRegistered {
                device: device.clone(),
            }
        };
        let event = insert_event(&transaction, data, now, None)?;
        transaction.commit().map_err(sql_error)?;
        Ok((device, event))
    }

    fn list_devices(&self) -> Result<Vec<Device>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT id FROM devices ORDER BY sort_name COLLATE NOCASE, id")
            .map_err(sql_error)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        ids.iter()
            .map(|id| {
                Self::device_from_row(&connection, id)?.ok_or_else(|| {
                    ApplicationError::Infrastructure("device index is inconsistent".into())
                })
            })
            .collect()
    }

    fn list_devices_page(
        &self,
        request: DevicePageRequest,
    ) -> Result<DevicePage, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        if let Some(cursor) = &request.cursor {
            let exists: Option<i64> = connection
                .query_row(
                    "SELECT 1 FROM devices WHERE id = ?1",
                    params![cursor.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if exists.is_none() {
                return Err(ApplicationError::Validation(
                    "device cursor is stale".into(),
                ));
            }
        }
        let status = request.status.as_ref().map(device_status_storage);
        let cursor = request.cursor.as_ref().map(ToString::to_string);
        let mut statement = connection
            .prepare(
                "SELECT id FROM devices
                 WHERE (?1 IS NULL OR status = ?1)
                   AND (?2 IS NULL OR (sort_name COLLATE NOCASE, id) >
                       (SELECT sort_name COLLATE NOCASE, id FROM devices WHERE id = ?2))
                 ORDER BY sort_name COLLATE NOCASE, id
                 LIMIT ?3",
            )
            .map_err(sql_error)?;
        let mut ids = statement
            .query_map(params![status, cursor, (request.limit + 1) as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        let has_more = ids.len() > request.limit;
        ids.truncate(request.limit);
        let next_cursor = has_more.then(|| {
            DeviceId(
                uuid::Uuid::parse_str(ids.last().expect("page has a last item"))
                    .expect("persisted device identifier is a UUID"),
            )
        });
        let devices = ids
            .iter()
            .map(|id| {
                Self::device_from_row(&connection, id)?.ok_or_else(|| {
                    ApplicationError::Infrastructure("device index is inconsistent".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DevicePage {
            devices,
            next_cursor,
        })
    }

    fn rename_device(
        &self,
        id: &DeviceId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let Some(payload) = transaction
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
        else {
            return Err(ApplicationError::EntityNotFound);
        };
        let mut device: Device = serde_json::from_str(&payload).map_err(json_error)?;
        device.name = name;
        transaction
            .execute(
                "UPDATE devices SET sort_name = ?1, payload = ?2 WHERE id = ?3",
                params![&device.name, to_json(&device)?, id.to_string()],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::DeviceUpdated {
                device: device.clone(),
            },
            now,
            None,
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok((device, event))
    }

    fn remove_device(
        &self,
        id: &DeviceId,
        now: DateTime<Utc>,
    ) -> Result<(Device, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let Some(payload) = transaction
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
        else {
            return Err(ApplicationError::EntityNotFound);
        };
        let mut device: Device = serde_json::from_str(&payload).map_err(json_error)?;
        device.status = DeviceStatus::Removed;
        transaction
            .execute(
                "UPDATE devices SET status = ?1, payload = ?2 WHERE id = ?3",
                params![
                    device_status_storage(&device.status),
                    to_json(&device)?,
                    id.to_string()
                ],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::DeviceRemoved {
                device: device.clone(),
            },
            now,
            None,
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok((device, event))
    }

    fn get_entity(&self, id: &EntityId) -> Result<Option<Entity>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .query_row(
                "SELECT payload FROM entities WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    fn rename_entity(
        &self,
        id: &EntityId,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Entity, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let Some((device_id, payload)) = transaction
            .query_row(
                "SELECT device_id, payload FROM entities WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(sql_error)?
        else {
            return Err(ApplicationError::EntityNotFound);
        };
        let mut entity: Entity = serde_json::from_str(&payload).map_err(json_error)?;
        entity.name = name;
        transaction
            .execute(
                "UPDATE entities SET payload = ?1 WHERE id = ?2",
                params![to_json(&entity)?, id.to_string()],
            )
            .map_err(sql_error)?;
        let device_payload: String = transaction
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![&device_id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let mut device: Device = serde_json::from_str(&device_payload).map_err(json_error)?;
        let Some(persisted) = device
            .entities
            .iter_mut()
            .find(|candidate| candidate.id == entity.id)
        else {
            return Err(ApplicationError::Infrastructure(
                "device and entity records are inconsistent".into(),
            ));
        };
        *persisted = entity.clone();
        transaction
            .execute(
                "UPDATE devices SET payload = ?1 WHERE id = ?2",
                params![to_json(&device)?, device_id],
            )
            .map_err(sql_error)?;
        let event = insert_event(&transaction, EventData::DeviceUpdated { device }, now, None)?;
        transaction.commit().map_err(sql_error)?;
        Ok((entity, event))
    }

    fn is_entity_commandable(&self, id: &EntityId) -> Result<bool, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT devices.payload FROM entities JOIN devices ON devices.id = entities.device_id WHERE entities.id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        payload
            .map(|payload| serde_json::from_str::<Device>(&payload).map_err(json_error))
            .transpose()
            .map(|device| device.is_some_and(|device| device.status != DeviceStatus::Removed))
    }

    fn get_entity_state(&self, id: &EntityId) -> Result<Vec<StateProperty>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT payload FROM entity_state WHERE entity_id = ?1 ORDER BY property_key")
            .map_err(sql_error)?;
        statement
            .query_map(params![id.to_string()], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .collect()
    }

    fn create_area(
        &self,
        name: String,
        now: DateTime<Utc>,
    ) -> Result<(Area, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let area = Area {
            id: AreaId::new(),
            name,
        };
        transaction
            .execute(
                "INSERT INTO areas (id, name) VALUES (?1, ?2)",
                params![area.id.to_string(), &area.name],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::AreaCreated { area: area.clone() },
            now,
            None,
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok((area, event))
    }

    fn list_areas(&self) -> Result<Vec<Area>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT id, name FROM areas ORDER BY name COLLATE NOCASE, id")
            .map_err(sql_error)?;
        statement
            .query_map([], |row| {
                Ok(Area {
                    id: AreaId(
                        uuid::Uuid::parse_str(&row.get::<_, String>(0)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    ),
                    name: row.get(1)?,
                })
            })
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)
    }

    fn assign_entity_area(
        &self,
        entity_id: &EntityId,
        area_id: Option<&AreaId>,
        now: DateTime<Utc>,
    ) -> Result<(Entity, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        if let Some(area_id) = area_id {
            let exists: i64 = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM areas WHERE id = ?1)",
                    params![area_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            if exists == 0 {
                return Err(ApplicationError::Validation("area does not exist".into()));
            }
        }
        let Some((device_id, payload)) = transaction
            .query_row(
                "SELECT device_id, payload FROM entities WHERE id = ?1",
                params![entity_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(sql_error)?
        else {
            return Err(ApplicationError::EntityNotFound);
        };
        let mut entity: Entity = serde_json::from_str(&payload).map_err(json_error)?;
        entity.area_id = area_id.cloned();
        transaction
            .execute(
                "UPDATE entities SET payload = ?1 WHERE id = ?2",
                params![to_json(&entity)?, entity.id.to_string()],
            )
            .map_err(sql_error)?;
        let device_payload: String = transaction
            .query_row(
                "SELECT payload FROM devices WHERE id = ?1",
                params![&device_id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let mut device: Device = serde_json::from_str(&device_payload).map_err(json_error)?;
        let Some(persisted) = device
            .entities
            .iter_mut()
            .find(|candidate| candidate.id == entity.id)
        else {
            return Err(ApplicationError::Infrastructure(
                "device and entity records are inconsistent".into(),
            ));
        };
        *persisted = entity.clone();
        transaction
            .execute(
                "UPDATE devices SET payload = ?1 WHERE id = ?2",
                params![to_json(&device)?, device_id],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::EntityAreaAssigned {
                entity: entity.clone(),
            },
            now,
            None,
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok((entity, event))
    }

    fn upsert_adapter_health(
        &self,
        health: AdapterHealth,
    ) -> Result<EventEnvelope, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        transaction
            .execute(
                "INSERT INTO adapter_health (adapter_id, payload) VALUES (?1, ?2) ON CONFLICT(adapter_id) DO UPDATE SET payload = excluded.payload",
                params![&health.adapter_id.0, to_json(&health)?],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::AdapterHealthChanged {
                health: health.clone(),
            },
            health.observed_at,
            None,
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok(event)
    }

    fn list_adapter_health(&self) -> Result<Vec<AdapterHealth>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT payload FROM adapter_health ORDER BY adapter_id")
            .map_err(sql_error)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .collect()
    }

    fn create_flow(
        &self,
        flow: FlowDefinition,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        transaction
            .execute(
                "INSERT INTO flows (id, payload) VALUES (?1, ?2)",
                params![flow.id.to_string(), to_json(&flow)?],
            )
            .map_err(sql_error)?;
        let event = insert_event(&transaction, EventData::FlowCreated { flow }, now, None)?;
        transaction.commit().map_err(sql_error)?;
        Ok(event)
    }

    fn update_flow(
        &self,
        flow: FlowDefinition,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        if transaction
            .execute(
                "UPDATE flows SET payload = ?1 WHERE id = ?2",
                params![to_json(&flow)?, flow.id.to_string()],
            )
            .map_err(sql_error)?
            != 1
        {
            return Err(ApplicationError::Infrastructure(
                "flow does not exist".into(),
            ));
        }
        let event = insert_event(&transaction, EventData::FlowUpdated { flow }, now, None)?;
        transaction.commit().map_err(sql_error)?;
        Ok(event)
    }

    fn get_flow(&self, id: &FlowId) -> Result<Option<FlowDefinition>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .query_row(
                "SELECT payload FROM flows WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    fn list_flows(&self) -> Result<Vec<FlowDefinition>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT payload FROM flows ORDER BY id")
            .map_err(sql_error)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .collect()
    }

    fn save_flow_run(&self, run: FlowRun) -> Result<(), ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        save_flow_run_sql(&connection, &run)?;
        Ok(())
    }

    fn admit_flow_run(
        &self,
        mut run: FlowRun,
        policy: &robine_flow_plan::ConcurrencyPolicy,
    ) -> Result<FlowAdmission, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let mut statement = transaction
            .prepare("SELECT payload FROM flow_runs WHERE flow_id = ?1 ORDER BY wake_at, id")
            .map_err(sql_error)?;
        let existing = statement
            .query_map(params![run.flow_id.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|payload| serde_json::from_str::<FlowRun>(&payload).map_err(json_error))
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let admission = match policy.mode {
            ConcurrencyMode::Single if !existing.is_empty() => FlowAdmission::Rejected {
                active_runs: existing.len(),
            },
            ConcurrencyMode::Single => {
                save_flow_run_sql(&transaction, &run)?;
                FlowAdmission::Start
            }
            ConcurrencyMode::Restart => {
                for previous in &existing {
                    transaction
                        .execute(
                            "DELETE FROM flow_runs WHERE id = ?1",
                            params![previous.id.to_string()],
                        )
                        .map_err(sql_error)?;
                }
                save_flow_run_sql(&transaction, &run)?;
                FlowAdmission::Restarted {
                    cancelled: existing,
                }
            }
            ConcurrencyMode::Queue if existing.len() >= policy.max_runs as usize => {
                FlowAdmission::Rejected {
                    active_runs: existing.len(),
                }
            }
            ConcurrencyMode::Queue if existing.is_empty() => {
                save_flow_run_sql(&transaction, &run)?;
                FlowAdmission::Start
            }
            ConcurrencyMode::Queue => {
                run.queued = true;
                save_flow_run_sql(&transaction, &run)?;
                FlowAdmission::Queued
            }
        };
        transaction.commit().map_err(sql_error)?;
        Ok(admission)
    }

    fn dequeue_next_flow_run(&self) -> Result<Option<FlowRun>, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let mut statement = transaction
            .prepare(
                "SELECT payload FROM flow_runs WHERE queued = 1 ORDER BY wake_at, id LIMIT 100",
            )
            .map_err(sql_error)?;
        let queued = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|payload| serde_json::from_str::<FlowRun>(&payload).map_err(json_error))
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for mut candidate in queued {
            let active: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM flow_runs WHERE flow_id = ?1 AND queued = 0",
                    params![candidate.flow_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            if active != 0 {
                continue;
            }
            candidate.queued = false;
            let changed = transaction
                .execute(
                    "UPDATE flow_runs SET queued = 0, payload = ?1 WHERE id = ?2 AND queued = 1",
                    params![to_json(&candidate)?, candidate.id.to_string()],
                )
                .map_err(sql_error)?;
            if changed == 1 {
                transaction.commit().map_err(sql_error)?;
                return Ok(Some(candidate));
            }
        }
        transaction.commit().map_err(sql_error)?;
        Ok(None)
    }

    fn due_flow_runs(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<FlowRun>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare(
                "SELECT payload FROM flow_runs WHERE queued = 0 AND wake_at <= ?1 ORDER BY wake_at, id LIMIT ?2",
            )
            .map_err(sql_error)?;
        statement
            .query_map(params![now.to_rfc3339(), limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .collect()
    }

    fn awaiting_flow_runs(&self, limit: usize) -> Result<Vec<FlowRun>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT payload FROM flow_runs WHERE queued = 0 AND awaiting = 1 ORDER BY id LIMIT ?1")
            .map_err(sql_error)?;
        statement
            .query_map(params![limit as i64], |row| row.get::<_, String>(0))
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .collect()
    }

    fn delete_flow_run(&self, id: &FlowRunId) -> Result<(), ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .execute(
                "DELETE FROM flow_runs WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(sql_error)?;
        Ok(())
    }

    fn save_flow_trace(
        &self,
        id: &FlowRunId,
        flow_id: &FlowId,
        result: serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection.execute(
            "INSERT INTO flow_run_traces (id, flow_id, recorded_at, payload) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET recorded_at = excluded.recorded_at, payload = excluded.payload",
            params![id.to_string(), flow_id.to_string(), now.to_rfc3339(), serde_json::to_string(&result).map_err(json_error)?],
        ).map_err(sql_error)?;
        Ok(())
    }

    fn get_flow_trace(
        &self,
        id: &FlowRunId,
    ) -> Result<Option<serde_json::Value>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .query_row(
                "SELECT payload FROM flow_run_traces WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|payload| serde_json::from_str(&payload).map_err(json_error))
            .transpose()
    }

    fn list_flow_traces(
        &self,
        flow_id: &FlowId,
        limit: usize,
    ) -> Result<Vec<FlowRunTrace>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection
            .prepare(
                "SELECT id, recorded_at, payload FROM flow_run_traces
                 WHERE flow_id = ?1 ORDER BY recorded_at DESC, id DESC LIMIT ?2",
            )
            .map_err(sql_error)?;
        statement
            .query_map(params![flow_id.to_string(), limit as i64], |row| {
                let id: String = row.get(0)?;
                let recorded_at: String = row.get(1)?;
                let payload: String = row.get(2)?;
                Ok((id, recorded_at, payload))
            })
            .map_err(sql_error)?
            .map(|row| {
                let (id, recorded_at, payload) = row.map_err(sql_error)?;
                Ok(FlowRunTrace {
                    id: FlowRunId(
                        uuid::Uuid::parse_str(&id)
                            .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?,
                    ),
                    flow_id: flow_id.clone(),
                    recorded_at: chrono::DateTime::parse_from_rfc3339(&recorded_at)
                        .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?
                        .with_timezone(&Utc),
                    result: serde_json::from_str(&payload).map_err(json_error)?,
                })
            })
            .collect()
    }

    fn claim_flow_trigger(
        &self,
        flow_id: &FlowId,
        correlation_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        // Une chaîne est conservée assez longtemps pour les rejeux, puis purgée
        // afin que le journal de déduplication reste borné.
        transaction
            .execute(
                "DELETE FROM flow_trigger_claims WHERE claimed_at < ?1",
                params![(now - chrono::Duration::days(30)).to_rfc3339()],
            )
            .map_err(sql_error)?;
        let already_claimed: i64 = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM flow_trigger_claims WHERE flow_id = ?1 AND correlation_id = ?2)",
                params![flow_id.to_string(), correlation_id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if already_claimed != 0 {
            transaction.commit().map_err(sql_error)?;
            return Ok(false);
        }
        let depth: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM flow_trigger_claims WHERE correlation_id = ?1",
                params![correlation_id],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        if depth >= 32 {
            return Err(ApplicationError::Validation(
                "Flow causal chain exceeded the maximum depth of 32".into(),
            ));
        }
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO flow_trigger_claims (flow_id, correlation_id, claimed_at) VALUES (?1, ?2, ?3)",
                params![flow_id.to_string(), correlation_id, now.to_rfc3339()],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(inserted == 1)
    }

    fn apply_reported_state(
        &self,
        state: ReportedState,
        now: DateTime<Utc>,
    ) -> Result<Vec<EventEnvelope>, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let current: Option<StateProperty> = transaction
            .query_row(
                "SELECT payload FROM entity_state WHERE entity_id = ?1 AND property_key = ?2",
                params![state.entity_id.to_string(), &state.key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()?;
        if current
            .as_ref()
            .is_some_and(|current| current.source_at > state.source_at)
        {
            return Ok(Vec::new());
        }
        let property = StateProperty {
            entity_id: state.entity_id,
            key: state.key,
            value: state.value,
            quality: StateQuality::Reported,
            source_at: state.source_at,
            received_at: now,
            version: current.map_or(1, |value| value.version + 1),
        };
        transaction.execute("INSERT INTO entity_state (entity_id, property_key, source_at, payload) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(entity_id, property_key) DO UPDATE SET source_at = excluded.source_at, payload = excluded.payload", params![property.entity_id.to_string(), &property.key, property.source_at.to_rfc3339(), to_json(&property)?]).map_err(sql_error)?;
        let state_event = insert_event(
            &transaction,
            EventData::StateReported {
                state: property.clone(),
            },
            now,
            None,
        )?;
        let mut events = vec![state_event];
        let mut statement = transaction
            .prepare("SELECT id, payload FROM commands")
            .map_err(sql_error)?;
        let commands = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        drop(statement);
        for (id, payload) in commands {
            let mut command: Command = serde_json::from_str(&payload).map_err(json_error)?;
            if command.entity_id != property.entity_id
                || command.key != property.key
                || command.value != property.value
                || !matches!(
                    command.status,
                    CommandStatus::Requested | CommandStatus::Dispatched
                )
            {
                continue;
            }
            command.status = CommandStatus::Confirmed;
            transaction
                .execute(
                    "UPDATE commands SET payload = ?1 WHERE id = ?2",
                    params![to_json(&command)?, id],
                )
                .map_err(sql_error)?;
            events.push(insert_event(
                &transaction,
                EventData::CommandConfirmed { command },
                now,
                None,
            )?);
        }
        transaction.commit().map_err(sql_error)?;
        Ok(events)
    }

    fn create_command(
        &self,
        command: Command,
    ) -> Result<(Command, EventEnvelope), ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        transaction
            .execute(
                "INSERT INTO commands (id, idempotency_key, payload) VALUES (?1, ?2, ?3)",
                params![
                    command.id.to_string(),
                    &command.idempotency_key,
                    to_json(&command)?
                ],
            )
            .map_err(sql_error)?;
        let event = insert_event(
            &transaction,
            EventData::CommandRequested {
                command: command.clone(),
            },
            command.requested_at,
            Some(command.correlation_id.clone()),
        )?;
        transaction.commit().map_err(sql_error)?;
        Ok((command, event))
    }

    fn find_command_by_idempotency(&self, key: &str) -> Result<Option<Command>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        connection
            .query_row(
                "SELECT payload FROM commands WHERE idempotency_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    fn transition_command(
        &self,
        id: &CommandId,
        status: CommandStatus,
        reason: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<EventEnvelope, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let payload: String = transaction
            .query_row(
                "SELECT payload FROM commands WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let mut command: Command = serde_json::from_str(&payload).map_err(json_error)?;
        command.status = status.clone();
        transaction
            .execute(
                "UPDATE commands SET payload = ?1 WHERE id = ?2",
                params![to_json(&command)?, id.to_string()],
            )
            .map_err(sql_error)?;
        let correlation_id = command.correlation_id.clone();
        let data = match status {
            CommandStatus::Dispatched => EventData::CommandDispatched { command },
            CommandStatus::Confirmed => EventData::CommandConfirmed { command },
            CommandStatus::Failed => EventData::CommandFailed {
                command,
                reason: reason.unwrap_or_else(|| "command failed".into()),
            },
            CommandStatus::Expired => EventData::CommandExpired { command },
            CommandStatus::Requested => EventData::CommandRequested { command },
        };
        let event = insert_event(&transaction, data, now, Some(correlation_id))?;
        transaction.commit().map_err(sql_error)?;
        Ok(event)
    }

    fn expire_commands(
        &self,
        before: DateTime<Utc>,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(sql_error)?;
        let mut statement = transaction
            .prepare("SELECT id, payload FROM commands LIMIT ?1")
            .map_err(sql_error)?;
        let rows = statement
            .query_map(params![limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        drop(statement);
        let mut events = Vec::new();
        for (id, payload) in rows {
            let mut command: Command = serde_json::from_str(&payload).map_err(json_error)?;
            if !matches!(
                command.status,
                CommandStatus::Requested | CommandStatus::Dispatched
            ) || command.requested_at >= before
            {
                continue;
            }
            command.status = CommandStatus::Expired;
            transaction
                .execute(
                    "UPDATE commands SET payload = ?1 WHERE id = ?2",
                    params![to_json(&command)?, id],
                )
                .map_err(sql_error)?;
            events.push(insert_event(
                &transaction,
                EventData::CommandExpired {
                    command: command.clone(),
                },
                now,
                Some(command.correlation_id),
            )?);
        }
        transaction.commit().map_err(sql_error)?;
        Ok(events)
    }

    fn events_after(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let mut statement = connection.prepare("SELECT sequence, occurred_at, correlation_id, payload FROM events WHERE sequence > ?1 ORDER BY sequence LIMIT ?2").map_err(sql_error)?;
        statement
            .query_map(params![after as i64, limit as i64], |row| {
                let occurred_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?
                    .with_timezone(&Utc);
                let payload: String = row.get(3)?;
                let data =
                    serde_json::from_str(&payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(EventEnvelope {
                    sequence: row.get::<_, i64>(0)? as u64,
                    occurred_at,
                    correlation_id: row.get(2)?,
                    data,
                })
            })
            .map_err(sql_error)?
            .collect::<Result<_, _>>()
            .map_err(sql_error)
    }

    fn recent_events(&self, limit: usize) -> Result<Vec<EventEnvelope>, ApplicationError> {
        recent_events_from_store(self, limit)
    }

    fn latest_event_sequence(&self) -> Result<u64, ApplicationError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
        let sequence: Option<i64> = connection
            .query_row("SELECT MAX(sequence) FROM events", [], |row| row.get(0))
            .map_err(sql_error)?;
        Ok(sequence.unwrap_or(0).max(0) as u64)
    }
}

impl EventStream for SqliteStore {
    fn publish(&self, event: EventEnvelope) {
        let _ = self.events.send(event);
    }
}

impl SqliteStore {
    /// Souscription éphémère d'infrastructure utilisée par les transports de push.
    /// La reprise durable passe toujours par `events_after` via le cas d'utilisation.
    pub fn subscribe_events(&self) -> broadcast::Receiver<EventEnvelope> {
        self.events.subscribe()
    }
}

fn existing_entities(
    transaction: &Transaction<'_>,
    device_id: &DeviceId,
) -> Result<std::collections::HashMap<String, Entity>, ApplicationError> {
    let mut statement = transaction
        .prepare("SELECT protocol_address, payload FROM entities WHERE device_id = ?1")
        .map_err(sql_error)?;
    let rows = statement
        .query_map(params![device_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_error)?;
    rows.map(|result| {
        let (address, payload) = result.map_err(sql_error)?;
        Ok((address, serde_json::from_str(&payload).map_err(json_error)?))
    })
    .collect()
}

fn recent_events_from_store(
    store: &SqliteStore,
    limit: usize,
) -> Result<Vec<EventEnvelope>, ApplicationError> {
    let connection = store
        .connection
        .lock()
        .map_err(|_| ApplicationError::Infrastructure("SQLite mutex poisoned".into()))?;
    let mut statement = connection
            .prepare("SELECT sequence, occurred_at, correlation_id, payload FROM events ORDER BY sequence DESC LIMIT ?1")
            .map_err(sql_error)?;
    let mut events = statement
        .query_map(params![limit as i64], |row| {
            let occurred_at = DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?
                .with_timezone(&Utc);
            let payload: String = row.get(3)?;
            let data = serde_json::from_str(&payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(EventEnvelope {
                sequence: row.get::<_, i64>(0)? as u64,
                occurred_at,
                correlation_id: row.get(2)?,
                data,
            })
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    events.reverse();
    Ok(events)
}

fn insert_event(
    transaction: &Transaction<'_>,
    data: EventData,
    now: DateTime<Utc>,
    correlation_id: Option<String>,
) -> Result<EventEnvelope, ApplicationError> {
    transaction
        .execute(
            "INSERT INTO events (occurred_at, correlation_id, payload) VALUES (?1, ?2, ?3)",
            params![now.to_rfc3339(), &correlation_id, to_json(&data)?],
        )
        .map_err(sql_error)?;
    Ok(EventEnvelope {
        sequence: transaction.last_insert_rowid() as u64,
        occurred_at: now,
        correlation_id,
        data,
    })
}
fn to_json<T: serde::Serialize>(value: &T) -> Result<String, ApplicationError> {
    serde_json::to_string(value).map_err(json_error)
}
fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), ApplicationError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sql_error)?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?
        .into_iter()
        .any(|name| name == column);
    if !exists {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))
            .map_err(sql_error)?;
    }
    Ok(())
}

fn device_status_storage(status: &DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Discovered => "discovered",
        DeviceStatus::Available => "available",
        DeviceStatus::Unavailable => "unavailable",
        DeviceStatus::Removed => "removed",
    }
}

fn json_error(error: serde_json::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
fn sql_error(error: rusqlite::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
fn token_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn normalize_mcp_scopes(requested_scopes: &[String]) -> Result<Vec<String>, ApplicationError> {
    let scopes = requested_scopes
        .iter()
        .map(|scope| match scope.trim() {
            "robine:read" => "read",
            "robine:control" => "control",
            "robine:automation:write" => "automation_write",
            "robine:admin" => "admin",
            scope => scope,
        })
        .collect::<std::collections::BTreeSet<_>>();
    if scopes.is_empty() {
        return Err(ApplicationError::Validation(
            "MCP token requires at least one scope".into(),
        ));
    }
    if scopes
        .iter()
        .any(|scope| !matches!(*scope, "read" | "control" | "automation_write" | "admin"))
    {
        return Err(ApplicationError::Validation("MCP scope is invalid".into()));
    }
    Ok(scopes.into_iter().map(str::to_owned).collect())
}
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("rob_{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn save_flow_run_sql(connection: &Connection, run: &FlowRun) -> Result<(), ApplicationError> {
    connection
        .execute(
            "INSERT INTO flow_runs (id, wake_at, awaiting, flow_id, queued, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                wake_at = excluded.wake_at,
                awaiting = excluded.awaiting,
                flow_id = excluded.flow_id,
                queued = excluded.queued,
                payload = excluded.payload",
            params![
                run.id.to_string(),
                run.wake_at.to_rfc3339(),
                i64::from(run.awaiting.is_some()),
                run.flow_id.to_string(),
                i64::from(run.queued),
                to_json(run)?
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use robine_application::{CommandDispatcher, FlowService, HomeRepository, HomeService};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingDispatcher(Mutex<Vec<Command>>);
    impl CommandDispatcher for RecordingDispatcher {
        fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
            self.0.lock().unwrap().push(command);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailFirstDispatcher(Mutex<(usize, Vec<Command>)>);
    impl CommandDispatcher for FailFirstDispatcher {
        fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
            let mut state = self.0.lock().unwrap();
            state.0 += 1;
            state.1.push(command);
            (state.0 > 1)
                .then_some(())
                .ok_or_else(|| ApplicationError::Infrastructure("temporary bridge failure".into()))
        }
    }

    struct AlwaysFailDispatcher;
    impl CommandDispatcher for AlwaysFailDispatcher {
        fn dispatch(&self, _command: Command) -> Result<(), ApplicationError> {
            Err(ApplicationError::Infrastructure(
                "bridge unavailable".into(),
            ))
        }
    }

    #[test]
    fn automation_engine_cursor_is_durable_and_never_moves_backwards() {
        let store = SqliteStore::open_in_memory().unwrap();
        assert_eq!(store.automation_engine_cursor().unwrap(), None);
        store.save_automation_engine_cursor(42).unwrap();
        store.save_automation_engine_cursor(7).unwrap();
        assert_eq!(store.automation_engine_cursor().unwrap(), Some(42));
    }

    #[test]
    fn retention_prunes_expired_event_trace_and_trigger_claim_in_bounded_batches() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let now = Utc.with_ymd_and_hms(2026, 8, 7, 12, 0, 0).unwrap();
        let expired_at = now - chrono::Duration::days(31);
        home.create_area("Ancien salon".into(), expired_at).unwrap();
        let flow_id = FlowId::new();
        let trace_id = FlowRunId::new();
        store
            .save_flow_trace(
                &trace_id,
                &flow_id,
                serde_json::json!({"kind": "completed"}),
                expired_at,
            )
            .unwrap();
        assert!(
            store
                .claim_flow_trigger(&flow_id, "expired-correlation", expired_at)
                .unwrap()
        );

        let purged = store.prune_retained_data(now, 10).unwrap();
        assert_eq!(
            purged,
            RetentionPurge {
                events: 1,
                flow_run_traces: 1,
                flow_trigger_claims: 1,
            }
        );
        assert!(store.events_after(0, 10).unwrap().is_empty());
        assert_eq!(store.get_flow_trace(&trace_id).unwrap(), None);
        assert!(
            store
                .claim_flow_trigger(&flow_id, "expired-correlation", now)
                .unwrap()
        );
    }

    #[test]
    fn persists_and_revisions_validated_flows() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let flows = FlowService::new(store.clone(), store.clone());
        let source = r#"(flow (meta :name "Soir paisible") (on (event :type "evening")) (do (audit :message "Bonne soirée")))"#;
        let created = flows.create(source.into(), true, Utc::now()).unwrap();
        assert_eq!(created.name, "Soir paisible");
        assert_eq!(flows.list().unwrap().len(), 1);
        let updated = flows
            .update(created.id.clone(), source.into(), false, Utc::now())
            .unwrap();
        assert_eq!(updated.revision, 2);
        assert!(!updated.enabled);
        assert_eq!(
            flows.get(&created.id).unwrap().source_hash,
            updated.source_hash
        );
    }

    #[test]
    fn scheduled_flow_executes_once_per_local_minute() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store);
        let flow = flows
            .create(
                "(flow (on (any-of (schedule :at \"09:15\" :weekdays [thu] :timezone \"Europe/Paris\") (event :type \"manual.wakeup\"))) (do (audit :message \"bonjour\")))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 15, 8, 15, 5).unwrap();
        let first = flows.execute_scheduled(&home, now);
        assert!(matches!(
            first.as_slice(),
            [Ok(robine_application::FlowExecution {
                flow_id,
                result: robine_flow_runtime::RunResult::Completed(_),
                ..
            })] if flow_id == &flow.id
        ));
        assert!(
            flows
                .execute_scheduled(&home, now + chrono::Duration::seconds(40))
                .is_empty()
        );
        assert!(
            flows
                .execute_scheduled(&home, Utc.with_ymd_and_hms(2026, 1, 16, 8, 15, 0).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn flow_single_rejects_a_second_active_execution_without_side_effects() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store);
        let flow = flows
            .create(
                "(flow (meta :mode :single) (on (event :type \"test\")) (do (wait 10s)))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        assert!(matches!(
            flows.execute_existing(&flow.id, &home, now).unwrap().result,
            robine_flow_runtime::RunResult::Suspended { .. }
        ));
        let second = flows.execute_existing(&flow.id, &home, now).unwrap();
        assert!(matches!(
            second.result,
            robine_flow_runtime::RunResult::Skipped(_)
        ));
        assert_eq!(
            flows.explain_run(&second.run_id.0.to_string()).unwrap()["status"],
            "skipped"
        );
    }

    #[test]
    fn flow_restart_cancels_the_previous_suspension_and_preserves_its_trace() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                "(flow (meta :mode :restart) (on (event :type \"test\")) (do (wait 10s)))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        let first = flows.execute_existing(&flow.id, &home, now).unwrap();
        let second = flows.execute_existing(&flow.id, &home, now).unwrap();
        assert!(matches!(
            second.result,
            robine_flow_runtime::RunResult::Suspended { .. }
        ));
        assert_eq!(
            flows.explain_run(&first.run_id.0.to_string()).unwrap()["status"],
            "cancelled"
        );
        assert_eq!(
            store
                .due_flow_runs(now + chrono::Duration::seconds(11), 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn flow_queue_is_durable_bounded_and_released_after_the_active_run() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                "(flow (meta :mode :queue :max-runs 2) (on (event :type \"test\")) (do (wait 1s)))"
                    .into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        let first = flows.execute_existing(&flow.id, &home, now).unwrap();
        let second = flows.execute_existing(&flow.id, &home, now).unwrap();
        let third = flows.execute_existing(&flow.id, &home, now).unwrap();
        assert!(matches!(
            first.result,
            robine_flow_runtime::RunResult::Suspended { .. }
        ));
        assert!(matches!(
            second.result,
            robine_flow_runtime::RunResult::Queued(_)
        ));
        assert!(matches!(
            third.result,
            robine_flow_runtime::RunResult::Skipped(_)
        ));
        let resumed = flows.resume_due(&home, now + chrono::Duration::seconds(2));
        assert!(resumed.iter().any(|execution| matches!(
            execution,
            Ok(robine_application::FlowExecution {
                result: robine_flow_runtime::RunResult::Completed(_),
                ..
            })
        )));
        assert!(resumed.iter().any(|execution| matches!(
            execution,
            Ok(robine_application::FlowExecution {
                result: robine_flow_runtime::RunResult::Suspended { .. },
                ..
            })
        )));
        assert_eq!(
            store
                .due_flow_runs(now + chrono::Duration::seconds(2), 10)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn flow_can_deactivate_an_existing_automation_through_the_application_port() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store);
        let target = flows
            .create(
                "(flow (on (event :type \"target\")) (do (audit :message \"target\")))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let controller = flows
            .create(
                format!(
                    "(flow (on (event :type \"controller\")) (do (deactivate (flow \"{}\"))))",
                    target.id
                ),
                true,
                Utc::now(),
            )
            .unwrap();
        let execution = flows
            .execute_existing(&controller.id, &home, Utc::now())
            .unwrap();
        assert!(!flows.get(&target.id).unwrap().enabled);
        let robine_flow_runtime::RunResult::Completed(trace) = execution.result else {
            panic!("controller flow should complete");
        };
        assert!(matches!(
            trace.steps.as_slice(),
            [robine_flow_runtime::TraceStep::AutomationChanged { flow_id, enabled: false }]
                if flow_id == &target.id.to_string()
        ));
    }

    #[test]
    fn flow_simulation_returns_the_same_resolved_branch_trace_without_side_effects() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let flows = FlowService::new(store.clone(), store);
        let flow = flows
            .create(
                "(flow (on (event :type \"simulation\")) (do (choose (= true true) (do (audit :message \"then\")) (do (audit :message \"else\")))))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let simulation = flows.simulate_existing(&flow.id).unwrap();
        assert_eq!(simulation.command_count, 0);
        let robine_flow_runtime::RunResult::Completed(trace) = simulation.result else {
            panic!("simulation should complete");
        };
        let messages = trace
            .steps
            .into_iter()
            .filter_map(|step| match step {
                robine_flow_runtime::TraceStep::Audit { message } => Some(message),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(messages, ["choose: then", "then"]);
    }

    #[test]
    fn stores_hue_configuration_without_an_application_key() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .save_hue_bridge(&HueBridgeConfiguration {
                authority: "192.168.1.4".into(),
                certificate_pem: "certificate".into(),
                certificate_sha256: "abc".into(),
                secret_name: "hue:192.168.1.4".into(),
            })
            .unwrap();
        let saved = store.list_hue_bridges().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].secret_name, "hue:192.168.1.4");
    }

    #[test]
    fn refuses_a_tampered_migration_history() {
        let root =
            std::env::temp_dir().join(format!("robine-migration-test-{}", uuid::Uuid::new_v4()));
        let store = SqliteStore::open(&root).unwrap();
        drop(store);
        let connection = rusqlite::Connection::open(&root).unwrap();
        connection
            .execute(
                "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
                [],
            )
            .unwrap();
        drop(connection);
        assert!(SqliteStore::open(&root).is_err());
        std::fs::remove_file(root).unwrap();
    }

    #[test]
    fn area_assignment_survives_a_rediscovery() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let discovery = DeviceDiscovery {
            adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
            protocol_address: "light-a".into(),
            name: "Lampe salon".into(),
            entities: vec![DiscoveryEntity {
                protocol_address: "light-a".into(),
                name: "Lampe salon".into(),
                kind: "light".into(),
                capabilities: vec![Capability::new("switch", 1).unwrap()],
            }],
        };
        let device = home
            .register_discovery(discovery.clone(), Utc::now())
            .unwrap();
        let area = home.create_area("Salon".into(), Utc::now()).unwrap();
        let entity = home
            .assign_entity_area(
                device.entities[0].id.clone(),
                Some(area.id.clone()),
                Utc::now(),
            )
            .unwrap();
        assert_eq!(entity.area_id, Some(area.id.clone()));

        let rediscovered = home.register_discovery(discovery, Utc::now()).unwrap();
        assert_eq!(rediscovered.entities[0].id, entity.id);
        assert_eq!(rediscovered.entities[0].area_id, Some(area.id));
        assert!(
            store
                .events_after(0, 20)
                .unwrap()
                .iter()
                .any(|event| { matches!(event.data, EventData::EntityAreaAssigned { .. }) })
        );
    }

    #[test]
    fn renamed_entities_survive_rediscovery_and_removed_devices_reject_commands() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let discovery = DeviceDiscovery {
            adapter_id: AdapterId::new("hue:bridge-a").unwrap(),
            protocol_address: "light-a".into(),
            name: "Nom Hue".into(),
            entities: vec![DiscoveryEntity {
                protocol_address: "light-a".into(),
                name: "Nom Hue".into(),
                kind: "light".into(),
                capabilities: vec![Capability::new("switch", 1).unwrap()],
            }],
        };
        let device = home
            .register_discovery(discovery.clone(), Utc::now())
            .unwrap();
        let entity_id = device.entities[0].id.clone();
        home.rename_device(device.id.clone(), "Lampe du salon".into(), Utc::now())
            .unwrap();
        home.rename_entity(entity_id.clone(), "Coin lecture".into(), Utc::now())
            .unwrap();
        let rediscovered = home.register_discovery(discovery, Utc::now()).unwrap();
        assert_eq!(rediscovered.name, "Lampe du salon");
        assert_eq!(rediscovered.entities[0].name, "Coin lecture");

        home.remove_device(rediscovered.id, Utc::now()).unwrap();
        let error = home
            .request_command(
                entity_id,
                "switch".into(),
                StateValue::Bool(true),
                "removed-device-command".into(),
                Utc::now(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("removed device"));
        assert!(dispatcher.0.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_reported_state_never_reaches_the_projection() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "switch-a".into(),
                    name: "Interrupteur".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "switch-a".into(),
                        name: "Interrupteur".into(),
                        kind: "switch".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity_id = device.entities[0].id.clone();
        assert!(
            home.apply_reported_state(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "light.brightness".into(),
                    value: StateValue::Percentage(40.0),
                    source_at: Utc::now(),
                },
                Utc::now(),
            )
            .is_err()
        );
        assert!(store.get_entity_state(&entity_id).unwrap().is_empty());
    }

    #[test]
    fn unconfirmed_commands_expire_and_emit_a_terminal_event() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "switch-a".into(),
                    name: "Interrupteur".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "switch-a".into(),
                        name: "Interrupteur".into(),
                        kind: "switch".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        let command = home
            .request_command(
                device.entities[0].id.clone(),
                "switch".into(),
                StateValue::Bool(true),
                "will-expire".into(),
                now,
            )
            .unwrap();
        assert_eq!(command.status, CommandStatus::Dispatched);
        assert_eq!(
            home.expire_stale_commands(
                now + chrono::Duration::seconds(31),
                chrono::Duration::seconds(30)
            )
            .unwrap(),
            1
        );
        assert!(store.events_after(0, 20).unwrap().iter().any(
            |event| matches!(&event.data, EventData::CommandExpired { command: expired } if expired.id == command.id)
        ));
    }

    #[test]
    fn enabled_flow_runs_once_for_its_matching_reported_state() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-1".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-1".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let source = format!(
            "(flow (on (state-changed (entity \"{entity}\") :property \"switch\" :to true)) (do (command (entity \"{entity}\") :turn-off)))"
        );
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows.create(source, true, Utc::now()).unwrap();
        let state = StateProperty {
            entity_id: entity,
            key: "switch".into(),
            value: StateValue::Bool(true),
            quality: StateQuality::Reported,
            source_at: Utc::now(),
            received_at: Utc::now(),
            version: 1,
        };
        let executions = flows.execute_state_triggered(&state, &home, Utc::now());
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].as_ref().unwrap().flow_id, flow.id);
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn enabled_flow_runs_for_a_matching_persisted_domain_event() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-1".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-1".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!(
                    "(flow (on (event :type \"adapter.health_changed\")) (do (command (entity \"{entity}\") :turn-on)))"
                ),
                true,
                Utc::now(),
            )
            .unwrap();
        home.update_adapter_health(AdapterHealth {
            adapter_id: AdapterId::new("test:adapter").unwrap(),
            status: AdapterStatus::Available,
            detail: None,
            observed_at: Utc::now(),
        })
        .unwrap();
        let event = store.events_after(0, 20).unwrap().pop().unwrap();
        let executions = flows.execute_event_triggered(&event, &home, Utc::now());
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].as_ref().unwrap().flow_id, flow.id);
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn flow_any_of_trigger_matches_each_supported_event_branch() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store.clone());
        flows
            .create(
                "(flow (on (any-of (event :type \"area.created\") (event :type \"device.created\"))) (do (audit :message \"vu\")))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let event = EventEnvelope {
            sequence: 1,
            occurred_at: Utc::now(),
            correlation_id: Some("cor-any-of".into()),
            data: EventData::AreaCreated {
                area: Area {
                    id: AreaId::new(),
                    name: "Salon".into(),
                },
            },
        };
        let executions = flows.execute_event_triggered(&event, &home, Utc::now());
        assert_eq!(executions.len(), 1);
        assert!(executions[0].is_ok());
    }

    #[test]
    fn event_trigger_does_not_reconsume_its_own_command_chain() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-1".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-1".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        flows
            .create(
                format!(
                    "(flow (on (event :type \"command.requested\")) (do (command (entity \"{entity}\") :turn-off)))"
                ),
                true,
                Utc::now(),
            )
            .unwrap();
        let external = home
            .request_command(
                entity,
                "switch".into(),
                StateValue::Bool(true),
                "external-command".into(),
                Utc::now(),
            )
            .unwrap();
        let initial = store
            .events_after(0, 50)
            .unwrap()
            .into_iter()
            .find(|event| {
                matches!(&event.data, EventData::CommandRequested { command } if command.id == external.id)
            })
            .unwrap();
        assert_eq!(
            flows
                .execute_event_triggered(&initial, &home, Utc::now())
                .len(),
            1
        );
        let emitted = store
            .events_after(initial.sequence, 50)
            .unwrap()
            .into_iter()
            .find(|event| {
                matches!(&event.data, EventData::CommandRequested { command } if command.id != external.id)
            })
            .unwrap();
        assert_eq!(emitted.correlation_id, initial.correlation_id);
        assert!(
            flows
                .execute_event_triggered(&emitted, &home, Utc::now())
                .is_empty()
        );
        assert_eq!(dispatcher.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn causal_chain_has_a_visible_global_depth_limit() {
        let store = SqliteStore::open_in_memory().unwrap();
        let now = Utc::now();
        for _ in 0..32 {
            assert!(
                store
                    .claim_flow_trigger(&FlowId::new(), "cor_test_chain", now)
                    .unwrap()
            );
        }
        assert!(matches!(
            store.claim_flow_trigger(&FlowId::new(), "cor_test_chain", now),
            Err(ApplicationError::Validation(message)) if message.contains("maximum depth of 32")
        ));
    }

    #[test]
    fn mcp_tokens_are_separate_from_api_tokens_and_expire() {
        let store = SqliteStore::open_in_memory().unwrap();
        let now = Utc::now();
        let (token, _) = store.issue_read_mcp_token(60, now).unwrap();
        assert!(store.authenticate_mcp_read(&token, now).unwrap());
        assert!(!store.authenticate(&token).unwrap());
        assert!(
            !store
                .authenticate_mcp_read(&token, now + chrono::Duration::seconds(61))
                .unwrap()
        );
    }

    #[test]
    fn mcp_control_approval_is_bound_to_one_token_and_one_exact_request() {
        let store = SqliteStore::open_in_memory().unwrap();
        let now = Utc::now();
        let (_, identity, _) = store
            .issue_mcp_token(&["read".into(), "control".into()], None, 60, now)
            .unwrap();
        let arguments_hash = "a".repeat(64);
        let (approval_id, _) = store
            .create_mcp_approval(
                &identity.token_id,
                "robine.command.request",
                &arguments_hash,
                30,
                now,
            )
            .unwrap();

        assert!(
            !store
                .consume_mcp_approval(
                    &identity.token_id,
                    "robine.command.request",
                    &"b".repeat(64),
                    &approval_id,
                    now,
                )
                .unwrap()
        );
        assert!(
            store
                .consume_mcp_approval(
                    &identity.token_id,
                    "robine.command.request",
                    &arguments_hash,
                    &approval_id,
                    now,
                )
                .unwrap()
        );
        assert!(
            !store
                .consume_mcp_approval(
                    &identity.token_id,
                    "robine.command.request",
                    &arguments_hash,
                    &approval_id,
                    now,
                )
                .unwrap()
        );
    }

    #[test]
    fn mcp_approval_requires_the_matching_token_scope() {
        let store = SqliteStore::open_in_memory().unwrap();
        let now = Utc::now();
        let (_, read_only, _) = store
            .issue_mcp_token(&["read".into()], None, 60, now)
            .unwrap();
        assert!(matches!(
            store.create_mcp_approval(
                &read_only.token_id,
                "robine.command.request",
                &"a".repeat(64),
                30,
                now,
            ),
            Err(ApplicationError::Validation(message)) if message.contains("required scope")
        ));
    }

    #[test]
    fn allow_listed_mcp_token_persists_its_targets_and_enforces_its_hourly_quota() {
        let store = SqliteStore::open_in_memory().unwrap();
        let now = Utc::now();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let policy = McpWritePolicy::AllowListed {
            commands: vec![robine_mcp_types::McpCommandAllowance {
                entity_id: entity_id.clone(),
                keys: vec!["switch".into()],
            }],
            max_commands_per_hour: 2,
        };
        let (token, identity, _) = store
            .issue_mcp_token(
                &["read".into(), "control".into()],
                Some(policy.clone()),
                60,
                now,
            )
            .unwrap();
        assert_eq!(identity.write_policy, policy);
        assert_eq!(
            store
                .authenticate_mcp(&token, now)
                .unwrap()
                .unwrap()
                .write_policy,
            policy
        );
        assert!(
            store
                .claim_mcp_allow_listed_command(
                    &identity.token_id,
                    "robine.command.request",
                    &"a".repeat(64),
                    2,
                    now,
                )
                .unwrap()
        );
        assert!(
            store
                .claim_mcp_allow_listed_command(
                    &identity.token_id,
                    "robine.command.request",
                    &"b".repeat(64),
                    2,
                    now,
                )
                .unwrap()
        );
        assert!(
            !store
                .claim_mcp_allow_listed_command(
                    &identity.token_id,
                    "robine.command.request",
                    &"c".repeat(64),
                    2,
                    now,
                )
                .unwrap()
        );
        assert!(matches!(
            store.create_mcp_approval(
                &identity.token_id,
                "robine.command.request",
                &"d".repeat(64),
                30,
                now,
            ),
            Err(ApplicationError::Validation(message)) if message.contains("do not use")
        ));
        assert!(matches!(
            store.issue_mcp_token(
                &["read".into(), "control".into()],
                Some(McpWritePolicy::AllowListed {
                    commands: vec![robine_mcp_types::McpCommandAllowance {
                        entity_id: "not-an-entity-id".into(),
                        keys: vec!["switch".into()],
                    }],
                    max_commands_per_hour: 1,
                }),
                60,
                now,
            ),
            Err(ApplicationError::Validation(message)) if message.contains("UUID")
        ));
    }

    #[test]
    fn suspended_flow_resumes_after_its_persisted_wait_without_repeating_command() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-1".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-1".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let source = format!(
            "(flow (on (event :type \"test\")) (do (command (entity \"{entity}\") :turn-on) (wait 1ms) (command (entity \"{entity}\") :turn-off)))"
        );
        let flow = flows.create(source, true, Utc::now()).unwrap();
        let now = Utc::now();
        assert!(matches!(
            flows.execute_existing(&flow.id, &home, now).unwrap().result,
            robine_flow_runtime::RunResult::Suspended { .. }
        ));
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
        assert_eq!(
            store
                .due_flow_runs(now + chrono::Duration::milliseconds(2), 10)
                .unwrap()
                .len(),
            1
        );
        let resumed = flows.resume_due(&home, now + chrono::Duration::milliseconds(2));
        let execution = resumed.into_iter().next().unwrap().unwrap();
        let robine_flow_runtime::RunResult::Completed(trace) = execution.result else {
            panic!("resumed Flow should complete");
        };
        assert!(matches!(
            trace.steps.as_slice(),
            [
                robine_flow_runtime::TraceStep::CommandRequested { verb: first, .. },
                robine_flow_runtime::TraceStep::Waiting { milliseconds: 1 },
                robine_flow_runtime::TraceStep::CommandRequested { verb: second, .. },
            ] if first == "turn-on" && second == "turn-off"
        ));
        assert_eq!(dispatcher.0.lock().unwrap().len(), 2);
        assert!(
            store
                .due_flow_runs(now + chrono::Duration::seconds(1), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn retry_backoff_persists_its_attempt_and_resumes_after_a_restart_boundary() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(FailFirstDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "retry-light".into(),
                    name: "Lampe retry".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "retry-light".into(),
                        name: "Lampe retry".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!(
                    "(flow (on (event :type \"test\")) (do (retry (command (entity \"{entity}\") :turn-on) :times 2 :backoff 1ms)))"
                ),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        assert!(matches!(
            flows.execute_existing(&flow.id, &home, now).unwrap().result,
            robine_flow_runtime::RunResult::Suspended {
                retry_attempt: Some(1),
                ..
            }
        ));
        let persisted = store
            .due_flow_runs(now + chrono::Duration::milliseconds(2), 10)
            .unwrap();
        assert!(matches!(
            persisted.as_slice(),
            [FlowRun {
                retry_attempt: Some(1),
                ..
            }]
        ));

        let resumed = flows.resume_due(&home, now + chrono::Duration::milliseconds(2));
        let execution = resumed.into_iter().next().unwrap().unwrap();
        assert!(matches!(
            execution.result,
            robine_flow_runtime::RunResult::Completed(robine_flow_runtime::RunTrace { steps })
                if matches!(steps.as_slice(), [
                    robine_flow_runtime::TraceStep::RetryScheduled { next_attempt: 2, total_attempts: 2, backoff_milliseconds: 1 },
                    robine_flow_runtime::TraceStep::CommandRequested { verb, .. },
                ] if verb == "turn-on")
        ));
        assert_eq!(dispatcher.0.lock().unwrap().0, 2);
        assert!(
            store
                .due_flow_runs(now + chrono::Duration::seconds(1), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn exhausted_retry_is_terminal_and_keeps_its_trace_without_a_live_run() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(store.clone(), store.clone(), Arc::new(AlwaysFailDispatcher));
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "failed-retry-light".into(),
                    name: "Lampe indisponible".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "failed-retry-light".into(),
                        name: "Lampe indisponible".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!(
                    "(flow (on (event :type \"test\")) (do (retry (command (entity \"{entity}\") :turn-on) :times 1 :backoff 1ms)))"
                ),
                true,
                Utc::now(),
            )
            .unwrap();
        let execution = flows.execute_existing(&flow.id, &home, Utc::now()).unwrap();
        assert!(matches!(
            execution.result,
            robine_flow_runtime::RunResult::Failed(robine_flow_runtime::RunTrace { steps })
                if matches!(steps.as_slice(), [robine_flow_runtime::TraceStep::RetryExhausted { attempts: 1 }])
        ));
        assert!(store.due_flow_runs(Utc::now(), 10).unwrap().is_empty());
        let trace = flows.explain_run(&execution.run_id.0.to_string()).unwrap();
        assert_eq!(trace["status"], "failed");
        assert_eq!(trace["steps"][0]["type"], "retry_exhausted");
    }

    #[test]
    fn a_suspended_flow_expires_at_its_persisted_max_runtime_deadline() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher);
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                "(flow (meta :max-runtime 1ms) (on (event :type \"test\")) (do (wait 10ms)))"
                    .into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        assert!(matches!(
            flows.execute_existing(&flow.id, &home, now).unwrap().result,
            robine_flow_runtime::RunResult::Suspended { .. }
        ));
        let resumed = flows.resume_due(&home, now + chrono::Duration::milliseconds(2));
        assert!(matches!(
            resumed.as_slice(),
            [Ok(robine_application::FlowExecution {
                result: robine_flow_runtime::RunResult::TimedOut(_),
                ..
            })]
        ));
        assert!(
            store
                .due_flow_runs(now + chrono::Duration::seconds(1), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn choose_freezes_only_the_selected_branch_into_the_executed_plan() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher);
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows.create(
            "(flow (on (event :type \"test\")) (do (choose (= true true) (do (audit :message \"then\")) (do (audit :message \"else\")))))".into(),
            true,
            Utc::now(),
        ).unwrap();
        let execution = flows.execute_existing(&flow.id, &home, Utc::now()).unwrap();
        let robine_flow_runtime::RunResult::Completed(trace) = execution.result else {
            panic!("choose should complete");
        };
        let messages = trace
            .steps
            .into_iter()
            .filter_map(|step| match step {
                robine_flow_runtime::TraceStep::Audit { message } => Some(message),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["choose: then", "then"]);
    }

    #[test]
    fn completed_flow_trace_is_persisted_for_later_explanation() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let home = HomeService::new(
            store.clone(),
            store.clone(),
            Arc::new(RecordingDispatcher::default()),
        );
        let flows = FlowService::new(store.clone(), store);
        let flow = flows
            .create(
                "(flow (on (event :type \"test\")) (do (audit :message \"done\")))".into(),
                true,
                Utc::now(),
            )
            .unwrap();
        let execution = flows.execute_existing(&flow.id, &home, Utc::now()).unwrap();
        let trace = flows.explain_run(&execution.run_id.0.to_string()).unwrap();
        assert_eq!(trace["status"], "completed");
        assert_eq!(trace["steps"][0]["message"], "done");
        let runs = flows.list_runs(&flow.id, 20).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id.0, execution.run_id.0);
        assert_eq!(runs[0].result["status"], "completed");
    }

    #[test]
    fn suspended_await_resumes_only_for_its_persisted_matching_event() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-await".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-await".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!("(flow (on (event :type \"test.start\")) (do (await (any-of (event :type \"area.created\") (state-changed (entity \"{entity}\") :property \"switch\" :to true))) (command (entity \"{entity}\") :turn-on)))"),
                true,
                Utc::now(),
            )
            .unwrap();
        assert!(matches!(
            flows
                .execute_existing(&flow.id, &home, Utc::now())
                .unwrap()
                .result,
            robine_flow_runtime::RunResult::Suspended {
                awaiting: Some(_),
                ..
            }
        ));
        assert_eq!(store.awaiting_flow_runs(10).unwrap().len(), 1);
        let event = EventEnvelope {
            sequence: 1,
            occurred_at: Utc::now(),
            correlation_id: Some("cor-await".into()),
            data: EventData::AreaCreated {
                area: Area {
                    id: AreaId::new(),
                    name: "Entrée".into(),
                },
            },
        };
        let resumed = flows.execute_event_triggered(&event, &home, Utc::now());
        assert_eq!(resumed.len(), 1);
        assert!(resumed[0].is_ok());
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
        assert!(store.awaiting_flow_runs(10).unwrap().is_empty());
    }

    #[test]
    fn suspended_await_timeout_rejoins_the_persisted_plan() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-timeout".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-timeout".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!("(flow (on (event :type \"test.start\")) (do (await (event :type \"area.created\") :timeout 1ms) (command (entity \"{entity}\") :turn-on)))"),
                true,
                Utc::now(),
            )
            .unwrap();
        let now = Utc::now();
        flows.execute_existing(&flow.id, &home, now).unwrap();
        assert!(store.due_flow_runs(now, 10).unwrap().is_empty());
        let resumed = flows.resume_due(&home, now + chrono::Duration::milliseconds(2));
        assert_eq!(resumed.len(), 1);
        assert!(resumed[0].is_ok());
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn suspended_state_await_resumes_on_the_matching_reported_value() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:adapter").unwrap(),
                    protocol_address: "light-state-await".into(),
                    name: "Lampe".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-state-await".into(),
                        name: "Lampe".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!("(flow (on (event :type \"test.start\")) (do (await (any-of (event :type \"area.created\") (state-changed (entity \"{entity}\") :property \"switch\" :to true))) (command (entity \"{entity}\") :turn-off)))"),
                true,
                Utc::now(),
            )
            .unwrap();
        flows.execute_existing(&flow.id, &home, Utc::now()).unwrap();
        let state = StateProperty {
            entity_id: entity,
            key: "switch".into(),
            value: StateValue::Bool(true),
            quality: StateQuality::Reported,
            source_at: Utc::now(),
            received_at: Utc::now(),
            version: 1,
        };
        let resumed = flows.execute_state_triggered(&state, &home, Utc::now());
        assert_eq!(resumed.len(), 1);
        assert!(resumed[0].is_ok());
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_false_flow_guard_skips_the_run_before_any_command_is_dispatched() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let home = HomeService::new(store.clone(), store.clone(), dispatcher.clone());
        let device = home
            .register_discovery(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("test:guard").unwrap(),
                    protocol_address: "guard-light".into(),
                    name: "Lampe entrée".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "guard-light".into(),
                        name: "Lampe entrée".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                Utc::now(),
            )
            .unwrap();
        let entity = device.entities[0].id.clone();
        let flows = FlowService::new(store.clone(), store.clone());
        let flow = flows
            .create(
                format!("(flow (on (event :type \"test.guard\")) (when (= (state (entity \"{entity}\") :switch) true)) (do (command (entity \"{entity}\") :turn-on)))"),
                true,
                Utc::now(),
            )
            .unwrap();
        home.apply_reported_state(
            ReportedState {
                entity_id: entity.clone(),
                key: "switch".into(),
                value: StateValue::Bool(false),
                source_at: Utc::now(),
            },
            Utc::now(),
        )
        .unwrap();
        assert!(matches!(
            flows
                .execute_existing(&flow.id, &home, Utc::now())
                .unwrap()
                .result,
            robine_flow_runtime::RunResult::Skipped(_)
        ));
        assert!(dispatcher.0.lock().unwrap().is_empty());

        home.apply_reported_state(
            ReportedState {
                entity_id: entity,
                key: "switch".into(),
                value: StateValue::Bool(true),
                source_at: Utc::now(),
            },
            Utc::now(),
        )
        .unwrap();
        assert!(matches!(
            flows
                .execute_existing(&flow.id, &home, Utc::now())
                .unwrap()
                .result,
            robine_flow_runtime::RunResult::Completed(_)
        ));
        assert_eq!(dispatcher.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn unsupported_guards_are_rejected_before_flow_activation() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let flows = FlowService::new(store.clone(), store);
        assert!(matches!(
            flows.create(
                "(flow (on (event :type \"test.guard\")) (when (changed? true)) (do (audit :message \"never\")))".into(),
                true,
                Utc::now(),
            ),
            Err(robine_application::FlowError::Validation(diagnostics))
                if diagnostics.iter().any(|diagnostic| diagnostic.code == "flow.guard_unsupported")
        ));
    }
}
