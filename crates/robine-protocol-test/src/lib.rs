//! Protocole fictif déterministe pour les tests d'intégration Robine.
//!
//! Il n'ouvre aucun socket et ne connaît aucun format externe : les tests
//! pilotent explicitement découvertes, états rapportés, indisponibilités et
//! pannes de transport, tout en passant par les mêmes cas d'utilisation que
//! les adaptateurs réels.

use chrono::{DateTime, Utc};
use robine_application::{ApplicationError, CommandDispatcher, HomeService};
use robine_domain::{
    AdapterHealth, AdapterId, AdapterStatus, Command, Device, DeviceDiscovery, DeviceStatus,
    EntityId, ReportedState,
};
use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const DEFAULT_ADAPTER_ID: &str = "test:protocol";

/// Dispatcher contrôlable. Une commande visant une entité non découverte est
/// refusée ; `fail_next_command` simule une panne transitoire à la frontière
/// du protocole sans muter le domaine directement.
#[derive(Default)]
pub struct TestProtocol {
    routes: Mutex<HashSet<EntityId>>,
    commands: Mutex<Vec<Command>>,
    fail_next: AtomicBool,
}

impl TestProtocol {
    pub fn register(&self, entity_id: EntityId) -> Result<(), ApplicationError> {
        self.routes
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("test protocol routes unavailable".into())
            })?
            .insert(entity_id);
        Ok(())
    }

    pub fn fail_next_command(&self) {
        self.fail_next.store(true, Ordering::Release);
    }

    pub fn commands(&self) -> Result<Vec<Command>, ApplicationError> {
        self.commands
            .lock()
            .map(|commands| commands.clone())
            .map_err(|_| {
                ApplicationError::Infrastructure("test protocol commands unavailable".into())
            })
    }
}

impl CommandDispatcher for TestProtocol {
    fn dispatch(&self, command: Command) -> Result<(), ApplicationError> {
        let route_exists = self
            .routes
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("test protocol routes unavailable".into())
            })?
            .contains(&command.entity_id);
        if !route_exists {
            return Err(ApplicationError::Infrastructure(
                "test protocol has no route for entity".into(),
            ));
        }
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err(ApplicationError::Infrastructure(
                "test protocol transient transport failure".into(),
            ));
        }
        self.commands
            .lock()
            .map_err(|_| {
                ApplicationError::Infrastructure("test protocol commands unavailable".into())
            })?
            .push(command);
        Ok(())
    }
}

/// Adaptateur de test relié aux cas d'utilisation. Il ne laisse aucun type de
/// protocole factice traverser la frontière applicative.
#[derive(Clone)]
pub struct TestAdapter {
    adapter_id: AdapterId,
    protocol: std::sync::Arc<TestProtocol>,
    service: HomeService,
}

impl TestAdapter {
    pub fn new(
        adapter_id: AdapterId,
        protocol: std::sync::Arc<TestProtocol>,
        service: HomeService,
    ) -> Self {
        Self {
            adapter_id,
            protocol,
            service,
        }
    }

    pub fn discover(
        &self,
        mut discovery: DeviceDiscovery,
        now: DateTime<Utc>,
    ) -> Result<Device, ApplicationError> {
        discovery.adapter_id = self.adapter_id.clone();
        let device = self.service.register_discovery(discovery, now)?;
        for entity in &device.entities {
            self.protocol.register(entity.id.clone())?;
        }
        self.service.update_adapter_health(AdapterHealth {
            adapter_id: self.adapter_id.clone(),
            status: AdapterStatus::Available,
            detail: None,
            observed_at: now,
        })?;
        Ok(device)
    }

    pub fn report(&self, state: ReportedState, now: DateTime<Utc>) -> Result<(), ApplicationError> {
        self.service.apply_reported_state(state, now)
    }

    pub fn set_available(
        &self,
        available: bool,
        now: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        self.service.set_adapter_devices_status(
            &self.adapter_id,
            if available {
                DeviceStatus::Available
            } else {
                DeviceStatus::Unavailable
            },
            now,
        )?;
        self.service.update_adapter_health(AdapterHealth {
            adapter_id: self.adapter_id.clone(),
            status: if available {
                AdapterStatus::Available
            } else {
                AdapterStatus::Degraded
            },
            detail: (!available).then_some("test protocol intentionally unavailable".into()),
            observed_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use robine_application::HomeRepository;
    use robine_domain::{Capability, DiscoveryEntity, StateValue};
    use robine_store_sqlite::SqliteStore;
    use std::sync::Arc;

    #[test]
    fn drives_discovery_state_commands_and_a_transient_failure() {
        let store = Arc::new(SqliteStore::open_in_memory().unwrap());
        let protocol = Arc::new(TestProtocol::default());
        let service = HomeService::new(store.clone(), store.clone(), protocol.clone());
        let adapter = TestAdapter::new(
            AdapterId::new(DEFAULT_ADAPTER_ID).unwrap(),
            protocol.clone(),
            service.clone(),
        );
        let now = Utc::now();
        let device = adapter
            .discover(
                DeviceDiscovery {
                    adapter_id: AdapterId::new("ignored-by-test-adapter").unwrap(),
                    protocol_address: "unit-1".into(),
                    name: "Unité de test".into(),
                    entities: vec![DiscoveryEntity {
                        protocol_address: "light-1".into(),
                        name: "Lampe de test".into(),
                        kind: "light".into(),
                        capabilities: vec![Capability::new("switch", 1).unwrap()],
                    }],
                },
                now,
            )
            .unwrap();
        let entity_id = device.entities[0].id.clone();

        adapter
            .report(
                ReportedState {
                    entity_id: entity_id.clone(),
                    key: "switch".into(),
                    value: StateValue::Bool(false),
                    source_at: now,
                },
                now,
            )
            .unwrap();
        service
            .request_command(
                entity_id.clone(),
                "switch".into(),
                StateValue::Bool(true),
                "test-on".into(),
                now,
            )
            .unwrap();
        assert_eq!(protocol.commands().unwrap().len(), 1);

        protocol.fail_next_command();
        assert!(
            service
                .request_command(
                    entity_id.clone(),
                    "switch".into(),
                    StateValue::Bool(false),
                    "test-fail".into(),
                    now
                )
                .is_err()
        );
        adapter.set_available(false, now).unwrap();
        assert_eq!(
            store.list_devices().unwrap()[0].status,
            DeviceStatus::Unavailable
        );
    }
}
