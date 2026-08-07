//! Magasin de secrets hors SQLite. Sur macOS, les clés sont confiées au
//! trousseau système plutôt qu'au fichier de données Robine.

use std::{collections::HashMap, process::Command, sync::Mutex};
use thiserror::Error;

pub trait SecretStore: Send + Sync {
    fn put(&self, name: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, name: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, name: &str) -> Result<(), SecretError>;
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret name is invalid")]
    InvalidName,
    #[error("the operating-system secret store failed: {0}")]
    Platform(String),
}

/// Implémentation déterministe réservée aux tests et aux exécutions éphémères.
#[derive(Default)]
pub struct MemorySecretStore(Mutex<HashMap<String, String>>);
impl SecretStore for MemorySecretStore {
    fn put(&self, name: &str, value: &str) -> Result<(), SecretError> {
        valid_name(name)?;
        self.0
            .lock()
            .map_err(|_| SecretError::Platform("memory lock poisoned".into()))?
            .insert(name.into(), value.into());
        Ok(())
    }
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        valid_name(name)?;
        Ok(self
            .0
            .lock()
            .map_err(|_| SecretError::Platform("memory lock poisoned".into()))?
            .get(name)
            .cloned())
    }
    fn delete(&self, name: &str) -> Result<(), SecretError> {
        valid_name(name)?;
        self.0
            .lock()
            .map_err(|_| SecretError::Platform("memory lock poisoned".into()))?
            .remove(name);
        Ok(())
    }
}

/// Utilise `security`, l'interface native du trousseau macOS. Aucune valeur de
/// secret n'est affichée dans une erreur ou un log.
pub struct MacOsKeychainSecretStore {
    service: String,
}
impl MacOsKeychainSecretStore {
    pub fn new(service: impl Into<String>) -> Result<Self, SecretError> {
        let service = service.into();
        valid_name(&service)?;
        Ok(Self { service })
    }
    fn command(&self, args: &[&str]) -> Result<std::process::Output, SecretError> {
        Command::new("security")
            .args(args)
            .output()
            .map_err(|error| SecretError::Platform(error.to_string()))
    }
}
impl SecretStore for MacOsKeychainSecretStore {
    fn put(&self, name: &str, value: &str) -> Result<(), SecretError> {
        valid_name(name)?;
        let output = self.command(&[
            "add-generic-password",
            "-U",
            "-a",
            name,
            "-s",
            &self.service,
            "-w",
            value,
        ])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SecretError::Platform(
                "could not save secret in macOS Keychain".into(),
            ))
        }
    }
    fn get(&self, name: &str) -> Result<Option<String>, SecretError> {
        valid_name(name)?;
        let output = self.command(&[
            "find-generic-password",
            "-a",
            name,
            "-s",
            &self.service,
            "-w",
        ])?;
        if output.status.success() {
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim_end().into(),
            ));
        }
        if output.status.code() == Some(44) {
            Ok(None)
        } else {
            Err(SecretError::Platform(
                "could not read secret from macOS Keychain".into(),
            ))
        }
    }
    fn delete(&self, name: &str) -> Result<(), SecretError> {
        valid_name(name)?;
        let output = self.command(&["delete-generic-password", "-a", name, "-s", &self.service])?;
        if output.status.success() || output.status.code() == Some(44) {
            Ok(())
        } else {
            Err(SecretError::Platform(
                "could not delete secret from macOS Keychain".into(),
            ))
        }
    }
}

fn valid_name(name: &str) -> Result<(), SecretError> {
    if name.trim().is_empty() || name.contains('\0') {
        Err(SecretError::InvalidName)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_store_never_persists_between_instances() {
        let store = MemorySecretStore::default();
        store.put("hue:bridge-a", "secret").unwrap();
        assert_eq!(
            store.get("hue:bridge-a").unwrap().as_deref(),
            Some("secret")
        );
        store.delete("hue:bridge-a").unwrap();
        assert_eq!(store.get("hue:bridge-a").unwrap(), None);
    }
}
