// SPDX-License-Identifier: Apache-2.0

//! Client keys (ADR-011, ADR-004 §4): `lo-` + 64 hex chars (32 random bytes). Only the
//! SHA-256 hash is stored; authentication is an in-memory hash-map lookup.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use sha2::{Digest, Sha256};

use crate::ClientId;

pub const KEY_PREFIX: &str = "lo-";
/// Longer credentials are rejected before hashing.
pub const MAX_KEY_LEN: usize = 256;
const RANDOM_BYTES: usize = 32;
const DISPLAY_PREFIX_LEN: usize = KEY_PREFIX.len() + 8;

/// A freshly generated key. `key` is shown to the user once and never stored.
pub struct GeneratedKey {
    pub key: String,
    /// First characters of the key, safe to display (e.g. `lo-3f9a1b2c`).
    pub display_prefix: String,
    pub hash: [u8; 32],
}

impl fmt::Debug for GeneratedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedKey")
            .field("key", &"[REDACTED]")
            .field("display_prefix", &self.display_prefix)
            .finish()
    }
}

pub fn generate_client_key() -> GeneratedKey {
    let mut bytes = [0u8; RANDOM_BYTES];
    rand::fill(&mut bytes);
    let mut key = String::with_capacity(KEY_PREFIX.len() + RANDOM_BYTES * 2);
    key.push_str(KEY_PREFIX);
    for b in bytes {
        key.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        key.push(char::from(b"0123456789abcdef"[usize::from(b & 0x0f)]));
    }
    bytes.fill(0);
    GeneratedKey {
        display_prefix: key[..DISPLAY_PREFIX_LEN].to_owned(),
        hash: hash_client_key(&key),
        key,
    }
}

pub fn hash_client_key(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("missing API key; send `Authorization: Bearer <key>` or `x-api-key: <key>`")]
    Missing,
    #[error("invalid API key")]
    Invalid,
}

/// Maps key hashes to client ids. Rebuilt by the control plane whenever clients change;
/// readers never block (ArcSwap).
#[derive(Debug, Default)]
pub struct ClientKeyIndex {
    entries: ArcSwap<HashMap<[u8; 32], ClientId>>,
}

impl ClientKeyIndex {
    pub fn new(entries: impl IntoIterator<Item = ([u8; 32], ClientId)>) -> Self {
        let index = Self::default();
        index.replace(entries);
        index
    }

    pub fn replace(&self, entries: impl IntoIterator<Item = ([u8; 32], ClientId)>) {
        self.entries.store(Arc::new(entries.into_iter().collect()));
    }

    pub fn len(&self) -> usize {
        self.entries.load().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Extracts the key from `Authorization: Bearer` (OpenAI style) or `x-api-key`
    /// (Anthropic style) and resolves it.
    pub fn authenticate(&self, headers: &HeaderMap) -> Result<ClientId, AuthError> {
        let key = extract_key(headers)?;
        if key.len() > MAX_KEY_LEN || !key.starts_with(KEY_PREFIX) {
            return Err(AuthError::Invalid);
        }
        self.entries
            .load()
            .get(&hash_client_key(key))
            .cloned()
            .ok_or(AuthError::Invalid)
    }
}

fn extract_key(headers: &HeaderMap) -> Result<&str, AuthError> {
    if let Some(value) = headers.get(AUTHORIZATION) {
        let value = value.to_str().map_err(|_| AuthError::Invalid)?;
        let (scheme, token) = value.split_once(' ').ok_or(AuthError::Invalid)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(AuthError::Invalid);
        }
        return Ok(token.trim());
    }
    if let Some(value) = headers.get("x-api-key") {
        return value
            .to_str()
            .map(str::trim)
            .map_err(|_| AuthError::Invalid);
    }
    Err(AuthError::Missing)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(name: &'static str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_str(value).expect("header"));
        h
    }

    #[test]
    fn generated_keys_have_expected_shape_and_are_unique() {
        let a = generate_client_key();
        let b = generate_client_key();
        assert_ne!(a.key, b.key);
        assert_eq!(a.key.len(), 3 + 64);
        assert!(a.key.starts_with("lo-"));
        assert!(a.key[3..].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(a.key.starts_with(&a.display_prefix));
        assert_eq!(a.display_prefix.len(), 11);
        assert_eq!(a.hash, hash_client_key(&a.key));
        assert!(!format!("{a:?}").contains(&a.key));
    }

    #[test]
    fn authenticates_bearer_and_x_api_key() {
        let key = generate_client_key();
        let index = ClientKeyIndex::new([(key.hash, ClientId::from("client-1"))]);

        let bearer = headers("authorization", &format!("Bearer {}", key.key));
        assert_eq!(index.authenticate(&bearer).as_deref(), Ok("client-1"));

        let lower = headers("authorization", &format!("bearer {}", key.key));
        assert_eq!(index.authenticate(&lower).as_deref(), Ok("client-1"));

        let anthropic = headers("x-api-key", &key.key);
        assert_eq!(index.authenticate(&anthropic).as_deref(), Ok("client-1"));
    }

    #[test]
    fn rejects_missing_wrong_and_malformed_keys() {
        let key = generate_client_key();
        let index = ClientKeyIndex::new([(key.hash, ClientId::from("c"))]);

        assert_eq!(
            index.authenticate(&HeaderMap::new()),
            Err(AuthError::Missing)
        );
        let other = generate_client_key();
        assert_eq!(
            index.authenticate(&headers("x-api-key", &other.key)),
            Err(AuthError::Invalid)
        );
        assert_eq!(
            index.authenticate(&headers("authorization", &format!("Basic {}", key.key))),
            Err(AuthError::Invalid)
        );
        assert_eq!(
            index.authenticate(&headers("authorization", "Bearer sk-not-ours")),
            Err(AuthError::Invalid)
        );
        let long = format!("lo-{}", "a".repeat(MAX_KEY_LEN));
        assert_eq!(
            index.authenticate(&headers("x-api-key", &long)),
            Err(AuthError::Invalid)
        );
    }

    #[test]
    fn replace_takes_effect_immediately() {
        let key = generate_client_key();
        let index = ClientKeyIndex::new([(key.hash, ClientId::from("c"))]);
        let h = headers("x-api-key", &key.key);
        assert!(index.authenticate(&h).is_ok());

        index.replace([]);
        assert!(index.is_empty());
        assert_eq!(index.authenticate(&h), Err(AuthError::Invalid));
    }
}
