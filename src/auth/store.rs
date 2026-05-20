use crate::config::AppPaths;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;

const SERVICE: &str = "teams-cli";
const CHUNK_SIZE: usize = 1000;

pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("***redacted***")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("***redacted***")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.clear();
    }
}

pub trait SecretStore {
    fn set(&self, account: &str, value: &SecretString) -> Result<(), keyring::Error>;
    fn get(&self, account: &str) -> Result<Option<SecretString>, keyring::Error>;
    fn delete(&self, account: &str) -> Result<(), keyring::Error>;
}

pub fn default_store(paths: &AppPaths) -> Box<dyn SecretStore> {
    if std::env::var("TEAMS_KEYRING_BACKEND").is_ok_and(|value| value == "file") {
        Box::new(FileStore::new(paths.secrets_file.clone()))
    } else {
        Box::new(KeyringStore)
    }
}

pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(SERVICE, account)
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, account: &str, value: &SecretString) -> Result<(), keyring::Error> {
        let _ = self.delete(account);
        if value.as_str().len() <= CHUNK_SIZE {
            Self::entry(account)?.set_password(value.as_str())?;
            return Ok(());
        }

        let chunks: Vec<&str> = value
            .as_str()
            .as_bytes()
            .chunks(CHUNK_SIZE)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
            .collect();
        Self::entry(&format!("{account}.__parts"))?.set_password(&chunks.len().to_string())?;
        for (index, chunk) in chunks.iter().enumerate() {
            Self::entry(&format!("{account}.part{index}"))?.set_password(chunk)?;
        }
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<SecretString>, keyring::Error> {
        match Self::entry(&format!("{account}.__parts"))?.get_password() {
            Ok(count_text) => {
                let count = count_text.parse::<usize>().unwrap_or(0);
                let mut value = String::new();
                for index in 0..count {
                    match Self::entry(&format!("{account}.part{index}"))?.get_password() {
                        Ok(part) => value.push_str(&part),
                        Err(_) => return Ok(None),
                    }
                }
                Ok(Some(SecretString::new(value)))
            }
            Err(_) => match Self::entry(account)?.get_password() {
                Ok(value) => Ok(Some(SecretString::new(value))),
                Err(_) => Ok(None),
            },
        }
    }

    fn delete(&self, account: &str) -> Result<(), keyring::Error> {
        if let Ok(count_text) = Self::entry(&format!("{account}.__parts"))?.get_password() {
            let count = count_text.parse::<usize>().unwrap_or(0);
            for index in 0..count {
                let _ = Self::entry(&format!("{account}.part{index}"))?.delete_credential();
            }
            let _ = Self::entry(&format!("{account}.__parts"))?.delete_credential();
        }
        let _ = Self::entry(account)?.delete_credential();
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileSecrets {
    entries: BTreeMap<String, String>,
}

pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> FileSecrets {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self, secrets: &FileSecrets) -> Result<(), keyring::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
        }
        let content = serde_json::to_string_pretty(secrets)
            .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
        fs::write(&self.path, content)
            .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))
    }
}

impl SecretStore for FileStore {
    fn set(&self, account: &str, value: &SecretString) -> Result<(), keyring::Error> {
        let mut secrets = self.load();
        let encoded = base64::engine::general_purpose::STANDARD.encode(value.as_str());
        secrets.entries.insert(account.to_string(), encoded);
        self.save(&secrets)
    }

    fn get(&self, account: &str) -> Result<Option<SecretString>, keyring::Error> {
        let secrets = self.load();
        let Some(encoded) = secrets.entries.get(account) else {
            return Ok(None);
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
        let value = String::from_utf8(decoded)
            .map_err(|error| keyring::Error::PlatformFailure(Box::new(error)))?;
        Ok(Some(SecretString::new(value)))
    }

    fn delete(&self, account: &str) -> Result<(), keyring::Error> {
        let mut secrets = self.load();
        secrets.entries.remove(account);
        self.save(&secrets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secret() {
        let secret = SecretString::new("actual-secret".to_string());
        assert!(!format!("{secret:?}").contains("actual-secret"));
    }

    #[test]
    fn file_store_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FileStore::new(dir.path().join("secrets.json"));
        store
            .set("token", &SecretString::new("abc123".to_string()))
            .expect("set");
        let loaded = store.get("token").expect("get").expect("some");
        assert_eq!(loaded.as_str(), "abc123");
    }
}
