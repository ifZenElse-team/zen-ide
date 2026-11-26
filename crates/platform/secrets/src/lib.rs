pub mod database;
#[cfg(target_os = "macos")]
mod keychain;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretId {
    pub service: String,
    pub account: String,
}

impl SecretId {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Result<Self, SecretError> {
        let id = Self { service: service.into(), account: account.into() };
        if id.service.is_empty() || id.account.is_empty() {
            return Err(SecretError::InvalidId);
        }
        Ok(id)
    }
}

impl fmt::Display for SecretId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} / {}", self.service, self.account)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretError {
    Denied,
    Unsupported,
    InvalidId,
    Failed(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => f.write_str("access to the secret was denied"),
            Self::Unsupported => f.write_str("secrets are not supported on this system"),
            Self::InvalidId => f.write_str("a secret needs a valid service and account"),
            Self::Failed(reason) => write!(f, "the secret store failed: {reason}"),
        }
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    fn get(&self, id: &SecretId) -> Result<Option<Secret>, SecretError>;
    fn set(&self, id: &SecretId, secret: &Secret) -> Result<(), SecretError>;
    fn delete(&self, id: &SecretId) -> Result<bool, SecretError>;
    fn contains(&self, id: &SecretId) -> Result<bool, SecretError> {
        self.get(id).map(|secret| secret.is_some())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedStore;

impl SecretStore for UnsupportedStore {
    fn get(&self, _: &SecretId) -> Result<Option<Secret>, SecretError> {
        Err(SecretError::Unsupported)
    }

    fn set(&self, _: &SecretId, _: &Secret) -> Result<(), SecretError> {
        Err(SecretError::Unsupported)
    }

    fn delete(&self, _: &SecretId) -> Result<bool, SecretError> {
        Err(SecretError::Unsupported)
    }
}

#[derive(Default)]
struct Memory {
    secrets: HashMap<SecretId, Secret>,
    denied: Vec<SecretId>,
    accesses: usize,
}

#[derive(Default)]
pub struct MemoryStore(Mutex<Memory>);

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn deny(&self, id: &SecretId) {
        self.lock().denied.push(id.clone());
    }

    pub fn accesses(&self) -> usize {
        self.lock().accesses
    }

    pub fn ids(&self) -> Vec<SecretId> {
        let mut ids: Vec<SecretId> = self.lock().secrets.keys().cloned().collect();
        ids.sort();
        ids
    }

    fn lock(&self) -> MutexGuard<'_, Memory> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn access(&self, id: &SecretId) -> Result<MutexGuard<'_, Memory>, SecretError> {
        let mut memory = self.lock();
        memory.accesses += 1;
        if memory.denied.contains(id) {
            return Err(SecretError::Denied);
        }
        Ok(memory)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, id: &SecretId) -> Result<Option<Secret>, SecretError> {
        Ok(self.access(id)?.secrets.get(id).cloned())
    }

    fn set(&self, id: &SecretId, secret: &Secret) -> Result<(), SecretError> {
        self.access(id)?.secrets.insert(id.clone(), secret.clone());
        Ok(())
    }

    fn delete(&self, id: &SecretId) -> Result<bool, SecretError> {
        Ok(self.access(id)?.secrets.remove(id).is_some())
    }
}

#[derive(Clone)]
enum Known {
    Present(Secret),
    Absent(Instant),
    Denied,
}

impl Known {
    fn stale(&self, lifetime: Duration) -> bool {
        matches!(self, Known::Absent(since) if since.elapsed() >= lifetime)
    }
}

type Slot = Arc<Mutex<Option<Known>>>;

const ABSENT_LIFETIME: Duration = Duration::from_secs(30);

pub struct CachedStore {
    inner: Arc<dyn SecretStore>,
    slots: Mutex<HashMap<SecretId, Slot>>,
    absent_lifetime: Duration,
}

impl CachedStore {
    pub fn new(inner: Arc<dyn SecretStore>) -> Self {
        Self::with_absent_lifetime(inner, ABSENT_LIFETIME)
    }

    pub fn with_absent_lifetime(inner: Arc<dyn SecretStore>, absent_lifetime: Duration) -> Self {
        Self { inner, slots: Mutex::new(HashMap::new()), absent_lifetime }
    }

    pub fn retry(&self, id: &SecretId) {
        let slot = self.slot(id);
        let mut known = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*known, Some(Known::Denied)) {
            *known = None;
        }
    }

    fn slot(&self, id: &SecretId) -> Slot {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(slots.entry(id.clone()).or_default())
    }
}

impl SecretStore for CachedStore {
    fn get(&self, id: &SecretId) -> Result<Option<Secret>, SecretError> {
        let slot = self.slot(id);
        let mut known = slot.lock().unwrap_or_else(PoisonError::into_inner);
        let found = match known.take().filter(|found| !found.stale(self.absent_lifetime)) {
            Some(found) => found,
            None => match self.inner.get(id) {
                Ok(Some(secret)) => Known::Present(secret),
                Ok(None) => Known::Absent(Instant::now()),
                Err(SecretError::Denied) => Known::Denied,
                Err(error) => return Err(error),
            },
        };
        *known = Some(found.clone());
        match found {
            Known::Present(secret) => Ok(Some(secret)),
            Known::Absent(_) => Ok(None),
            Known::Denied => Err(SecretError::Denied),
        }
    }

    fn set(&self, id: &SecretId, secret: &Secret) -> Result<(), SecretError> {
        let slot = self.slot(id);
        let mut known = slot.lock().unwrap_or_else(PoisonError::into_inner);
        self.inner.set(id, secret)?;
        *known = Some(Known::Present(secret.clone()));
        Ok(())
    }

    fn contains(&self, id: &SecretId) -> Result<bool, SecretError> {
        let slot = self.slot(id);
        let known = slot.lock().unwrap_or_else(PoisonError::into_inner);
        match known.as_ref().filter(|found| !found.stale(self.absent_lifetime)) {
            Some(Known::Present(_)) => Ok(true),
            Some(Known::Absent(_)) => Ok(false),
            Some(Known::Denied) | None => self.inner.contains(id),
        }
    }

    fn delete(&self, id: &SecretId) -> Result<bool, SecretError> {
        let slot = self.slot(id);
        let mut known = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if known.as_ref().is_some_and(|found| matches!(found, Known::Absent(_)) && !found.stale(self.absent_lifetime)) {
            return Ok(false);
        }
        let deleted = self.inner.delete(id)?;
        *known = Some(Known::Absent(Instant::now()));
        Ok(deleted)
    }
}

pub fn system() -> Arc<CachedStore> {
    static SYSTEM: OnceLock<Arc<CachedStore>> = OnceLock::new();
    Arc::clone(SYSTEM.get_or_init(|| Arc::new(CachedStore::new(platform_store()))))
}

#[cfg(target_os = "macos")]
fn platform_store() -> Arc<dyn SecretStore> {
    Arc::new(keychain::KeychainStore)
}

#[cfg(not(target_os = "macos"))]
fn platform_store() -> Arc<dyn SecretStore> {
    Arc::new(UnsupportedStore)
}

#[cfg(test)]
mod tests;
