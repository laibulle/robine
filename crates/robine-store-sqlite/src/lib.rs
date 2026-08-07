//! Adaptateur SQLite de Robine. Toutes les mutations sont sérialisées par cette
//! instance; l'API l'appelle depuis un worker bloquant Actix.

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use rand::RngCore;
use robine_application::{ApplicationError, EventStream, HomeRepository};
use robine_domain::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};
use tokio::sync::broadcast;

pub struct SqliteStore {
    connection: Mutex<Connection>,
    events: broadcast::Sender<EventEnvelope>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        let connection = Connection::open(path).map_err(sql_error)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS devices (id TEXT PRIMARY KEY, adapter_id TEXT NOT NULL, protocol_address TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(adapter_id, protocol_address));
             CREATE TABLE IF NOT EXISTS entities (id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id), protocol_address TEXT NOT NULL, payload TEXT NOT NULL, UNIQUE(device_id, protocol_address));
             CREATE TABLE IF NOT EXISTS areas (id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE);
             CREATE TABLE IF NOT EXISTS adapter_health (adapter_id TEXT PRIMARY KEY, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS flows (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS entity_state (entity_id TEXT NOT NULL REFERENCES entities(id), property_key TEXT NOT NULL, source_at TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(entity_id, property_key));
             CREATE TABLE IF NOT EXISTS events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, occurred_at TEXT NOT NULL, correlation_id TEXT, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS commands (id TEXT PRIMARY KEY, idempotency_key TEXT NOT NULL UNIQUE, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS administrators (id INTEGER PRIMARY KEY CHECK(id = 1), password_hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS api_tokens (token_hash TEXT PRIMARY KEY, created_at TEXT NOT NULL);",
        ).map_err(sql_error)?;
        let (events, _) = broadcast::channel(512);
        Ok(Self {
            connection: Mutex::new(connection),
            events,
        })
    }

    pub fn open_in_memory() -> Result<Self, ApplicationError> {
        Self::open(":memory:")
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
        let mut existing = existing_entities(&transaction, &id)?;
        let mut persisted_entities = Vec::with_capacity(discovery.entities.len());
        for announced in discovery.entities {
            let entity_id = existing
                .remove(&announced.protocol_address)
                .unwrap_or_else(EntityId::new);
            let entity = Entity {
                id: entity_id,
                name: announced.name,
                kind: announced.kind,
                capabilities: announced.capabilities,
                area_id: None,
            };
            persisted_entities.push((announced.protocol_address, entity));
        }
        let device = Device {
            id: id.clone(),
            adapter_id: discovery.adapter_id,
            protocol_address: discovery.protocol_address,
            name: discovery.name,
            status: DeviceStatus::Available,
            entities: persisted_entities
                .iter()
                .map(|(_, entity)| entity.clone())
                .collect(),
        };
        transaction.execute("INSERT INTO devices (id, adapter_id, protocol_address, payload) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(adapter_id, protocol_address) DO UPDATE SET payload = excluded.payload", params![id.to_string(), &device.adapter_id.0, &device.protocol_address, to_json(&device)?]).map_err(sql_error)?;
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
            .prepare("SELECT id FROM devices ORDER BY name COLLATE NOCASE, id")
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
        let data = match status {
            CommandStatus::Dispatched => EventData::CommandDispatched { command },
            CommandStatus::Confirmed => EventData::CommandConfirmed { command },
            CommandStatus::Failed => EventData::CommandFailed {
                command,
                reason: reason.unwrap_or_else(|| "command failed".into()),
            },
            CommandStatus::Requested => EventData::CommandRequested { command },
        };
        let event = insert_event(&transaction, data, now, None)?;
        transaction.commit().map_err(sql_error)?;
        Ok(event)
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
) -> Result<std::collections::HashMap<String, EntityId>, ApplicationError> {
    let mut statement = transaction
        .prepare("SELECT protocol_address, id FROM entities WHERE device_id = ?1")
        .map_err(sql_error)?;
    let rows = statement
        .query_map(params![device_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_error)?;
    rows.map(|result| {
        let (address, id) = result.map_err(sql_error)?;
        Ok((
            address,
            EntityId(
                uuid::Uuid::parse_str(&id)
                    .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?,
            ),
        ))
    })
    .collect()
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
fn json_error(error: serde_json::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
fn sql_error(error: rusqlite::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
fn token_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("rob_{}", URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::FlowService;
    use std::sync::Arc;

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
}
