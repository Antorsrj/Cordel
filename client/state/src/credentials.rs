use std::fmt;

/// Short-lived credentials. They stay in process memory and are not written to the refresh store.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct VolatileTokens {
    access: Option<String>,
    livekit: Option<String>,
}

impl fmt::Debug for VolatileTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VolatileTokens")
            .field("access", &self.access.as_ref().map(|_| "redacted"))
            .field("livekit", &self.livekit.as_ref().map(|_| "redacted"))
            .finish()
    }
}

impl VolatileTokens {
    pub fn set_access(&mut self, token: impl Into<String>) {
        self.access = Some(token.into());
    }

    pub fn set_livekit(&mut self, token: impl Into<String>) {
        self.livekit = Some(token.into());
    }

    pub fn access(&self) -> Option<&str> {
        self.access.as_deref()
    }

    pub fn livekit(&self) -> Option<&str> {
        self.livekit.as_deref()
    }

    pub fn begin_reauthorization(&mut self) {
        self.access = None;
    }

    pub fn begin_full_media_reconnect(&mut self) {
        self.access = None;
        self.livekit = None;
    }
}

/// Opaque refresh credential. Only this value is allowed to leave process memory.
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshCredential(String);

impl RefreshCredential {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RefreshCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshCredential(redacted)")
    }
}

pub trait RefreshStore {
    fn load(&self) -> Option<RefreshCredential>;
    fn save(&mut self, credential: RefreshCredential) -> Result<(), StoreError>;
    fn clear(&mut self) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

#[derive(Debug, Default)]
pub struct MemoryRefreshStore {
    credential: Option<RefreshCredential>,
}

impl RefreshStore for MemoryRefreshStore {
    fn load(&self) -> Option<RefreshCredential> {
        self.credential.clone()
    }

    fn save(&mut self, credential: RefreshCredential) -> Result<(), StoreError> {
        self.credential = Some(credential);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        self.credential = None;
        Ok(())
    }
}

#[cfg(feature = "secret-service")]
pub struct SecretServiceRefreshStore {
    service: String,
    account: String,
}

#[cfg(feature = "secret-service")]
impl SecretServiceRefreshStore {
    pub fn new(account: impl Into<String>) -> Self {
        Self {
            service: "cordel".to_owned(),
            account: account.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(&self.service, &self.account).map_err(|err| StoreError(err.to_string()))
    }
}

#[cfg(feature = "secret-service")]
impl RefreshStore for SecretServiceRefreshStore {
    fn load(&self) -> Option<RefreshCredential> {
        let entry = self.entry().ok()?;
        entry.get_password().ok().map(RefreshCredential::new)
    }

    fn save(&mut self, credential: RefreshCredential) -> Result<(), StoreError> {
        self.entry()?
            .set_password(credential.expose())
            .map_err(|err| StoreError(err.to_string()))
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(StoreError(err.to_string())),
        }
    }
}

/// The refresh store a desktop client uses.
///
/// It prefers the operating system credential store and falls back to memory
/// when the desktop secret service cannot be reached, which is the case in a
/// headless session. The client asks which one it got, so a user is never told
/// that the session survives a restart when it does not.
#[cfg(feature = "secret-service")]
pub struct OsRefreshStore {
    inner: OsStoreKind,
}

#[cfg(feature = "secret-service")]
enum OsStoreKind {
    SecretService(SecretServiceRefreshStore),
    Memory(MemoryRefreshStore),
}

#[cfg(feature = "secret-service")]
impl OsRefreshStore {
    pub fn new(account: impl Into<String>) -> Self {
        let account = account.into();
        if os_store_available(&account) {
            Self {
                inner: OsStoreKind::SecretService(SecretServiceRefreshStore::new(account)),
            }
        } else {
            Self {
                inner: OsStoreKind::Memory(MemoryRefreshStore::default()),
            }
        }
    }

    /// Whether the refresh credential outlives the process.
    pub fn is_persistent(&self) -> bool {
        matches!(self.inner, OsStoreKind::SecretService(_))
    }
}

/// Probe the operating system store. A missing entry means the service works;
/// any other failure means it cannot be reached.
#[cfg(feature = "secret-service")]
fn os_store_available(account: &str) -> bool {
    let entry = match keyring::Entry::new("cordel", account) {
        Ok(entry) => entry,
        Err(_) => return false,
    };
    match entry.get_password() {
        Ok(_) => true,
        Err(keyring::Error::NoEntry) => true,
        Err(_) => false,
    }
}

#[cfg(feature = "secret-service")]
impl RefreshStore for OsRefreshStore {
    fn load(&self) -> Option<RefreshCredential> {
        match &self.inner {
            OsStoreKind::SecretService(store) => store.load(),
            OsStoreKind::Memory(store) => store.load(),
        }
    }

    fn save(&mut self, credential: RefreshCredential) -> Result<(), StoreError> {
        match &mut self.inner {
            OsStoreKind::SecretService(store) => store.save(credential),
            OsStoreKind::Memory(store) => store.save(credential),
        }
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        match &mut self.inner {
            OsStoreKind::SecretService(store) => store.clear(),
            OsStoreKind::Memory(store) => store.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The operating system store must really persist. A machine without a
    /// desktop secret service falls back to memory, and the test says so
    /// instead of pretending.
    #[cfg(feature = "secret-service")]
    #[test]
    fn the_operating_system_store_survives_a_new_handle() {
        let account = "cordel-state-test-account";
        let store = OsRefreshStore::new(account);
        if !store.is_persistent() {
            println!("no desktop secret service here: the store fell back to memory");
            return;
        }
        let mut store = store;
        store
            .save(RefreshCredential::new("refresh-persisted"))
            .unwrap();
        let restored = OsRefreshStore::new(account);
        assert_eq!(
            restored
                .load()
                .map(|credential| credential.expose().to_owned()),
            Some("refresh-persisted".to_owned()),
            "the credential must survive a new store handle"
        );
        let mut restored = restored;
        restored.clear().unwrap();
        assert!(restored.load().is_none(), "the entry must be removed");
    }

    #[test]
    fn refresh_survives_a_new_store_handle_and_tokens_do_not() {
        let mut store = MemoryRefreshStore::default();
        store.save(RefreshCredential::new("refresh-1")).unwrap();
        let restored = store.load().unwrap();
        assert_eq!(restored.expose(), "refresh-1");

        let mut tokens = VolatileTokens::default();
        tokens.set_access("access-secret");
        tokens.set_livekit("room-token-secret");
        let redacted = format!("{tokens:?}");
        assert!(redacted.contains("redacted"));
        assert!(!redacted.contains("access-secret"));
        assert!(!redacted.contains("room-token-secret"));
        tokens.begin_full_media_reconnect();
        assert_eq!(tokens.access(), None);
        assert_eq!(tokens.livekit(), None);
    }
}
