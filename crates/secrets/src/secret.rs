// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::str::FromStr;

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

/// A secret value (API key, token). Zeroed on drop; never printed.
///
/// There is deliberately no `Display`, `Clone` or `Serialize` implementation: the only way to
/// read the value is [`Secret::expose`], which makes every use site explicit and greppable.
pub struct Secret(SecretString);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// Returns the raw value. Callers must not log, persist or embed it in errors.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    pub fn len(&self) -> usize {
        self.expose().len()
    }

    pub fn is_empty(&self) -> bool {
        self.expose().is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Opaque handle to a secret held in the OS secret store. This is what SQLite stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SecretRef(Uuid);

impl SecretRef {
    /// Generates a new random reference.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid secret reference")]
pub struct SecretRefParseError;

impl FromStr for SecretRef {
    type Err = SecretRefParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| SecretRefParseError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let secret = Secret::new("sk-live-very-secret-value");
        let debug = format!("{secret:?}");
        assert_eq!(debug, "Secret([REDACTED])");
        assert!(!debug.contains("very-secret"));

        let nested = format!("{:?}", Some(&secret));
        assert!(!nested.contains("very-secret"));
    }

    #[test]
    fn expose_returns_value() {
        let secret = Secret::from(String::from("abc"));
        assert_eq!(secret.expose(), "abc");
        assert_eq!(secret.len(), 3);
        assert!(!secret.is_empty());
    }

    #[test]
    fn secret_ref_roundtrips_through_string() {
        let r = SecretRef::generate();
        let parsed: SecretRef = r.to_string().parse().expect("valid ref");
        assert_eq!(parsed, r);
    }

    #[test]
    fn secret_ref_rejects_garbage() {
        assert!("not-a-uuid".parse::<SecretRef>().is_err());
        assert!("".parse::<SecretRef>().is_err());
    }
}
