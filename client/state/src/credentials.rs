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

#[cfg(test)]
mod tests {
    use super::*;

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
