//! Thin wrapper over the platform credential store (Secret Service on Linux,
//! Keychain on macOS, Credential Manager on Windows). Falls back gracefully
//! when no backend is available (headless / CI), in which case `load` returns
//! `None` and `store` returns an error.

const SERVICE: &str = "noip-duc";

/// Distinguishes the two secrets we hold per user.
pub enum Slot {
    Password,
    ApiKey,
}

impl Slot {
    fn target(&self) -> &'static str {
        match self {
            Slot::Password => "password",
            Slot::ApiKey => "api-key",
        }
    }
}

fn entry(account: &str, slot: &Slot) -> Result<keyring::Entry, String> {
    keyring::Entry::new_with_target(slot.target(), SERVICE, account).map_err(|e| e.to_string())
}

/// Persist a secret. Returns `Err` if no backend is available.
pub fn store(account: &str, slot: Slot, value: &str) -> Result<(), String> {
    let e = entry(account, &slot)?;
    e.set_password(value).map_err(|err| err.to_string())
}

/// Load a secret if present. Returns `None` if missing OR if no backend is
/// available (caller cannot distinguish, by design — both mean "no usable
/// remembered secret").
pub fn load(account: &str, slot: Slot) -> Option<String> {
    let e = entry(account, &slot).ok()?;
    e.get_password().ok()
}

/// Remove a secret. Silent no-op if it didn't exist or no backend is available.
pub fn delete(account: &str, slot: Slot) {
    if let Ok(e) = entry(account, &slot) {
        let _ = e.delete_credential();
    }
}
