// SPDX-License-Identifier: Apache-2.0

//! `X-Loadout-Session-ID` affinity (ADR-003 §5, ExecPlan 004 §3.5): a session keeps using
//! the account model it last used while that stays eligible. In memory only.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const DEFAULT_SESSION_CAPACITY: usize = 10_000;
pub const DEFAULT_SESSION_TTL_MS: i64 = 2 * 3600 * 1000;
/// Session ids longer than this are ignored.
pub const MAX_SESSION_ID_LEN: usize = 256;

#[derive(Debug)]
struct Entry {
    account_model: Arc<str>,
    used_ms: i64,
}

#[derive(Debug)]
pub struct SessionAffinity {
    capacity: usize,
    ttl_ms: i64,
    map: Mutex<HashMap<Box<str>, Entry>>,
}

impl Default for SessionAffinity {
    fn default() -> Self {
        Self::new(DEFAULT_SESSION_CAPACITY, DEFAULT_SESSION_TTL_MS)
    }
}

impl SessionAffinity {
    pub fn new(capacity: usize, ttl_ms: i64) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl_ms,
            map: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Box<str>, Entry>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The account model the session last used, if still fresh.
    pub fn get(&self, session: &str, now_ms: i64) -> Option<Arc<str>> {
        let map = self.lock();
        map.get(session)
            .filter(|e| now_ms - e.used_ms <= self.ttl_ms)
            .map(|e| e.account_model.clone())
    }

    pub fn put(&self, session: &str, account_model: Arc<str>, now_ms: i64) {
        if session.is_empty() || session.len() > MAX_SESSION_ID_LEN {
            return;
        }
        let mut map = self.lock();
        if let Some(e) = map.get_mut(session) {
            e.account_model = account_model;
            e.used_ms = now_ms;
            return;
        }
        if map.len() >= self.capacity {
            let ttl = self.ttl_ms;
            map.retain(|_, e| now_ms - e.used_ms <= ttl);
            if map.len() >= self.capacity {
                // Still full: drop the least recently used tenth.
                let mut ages: Vec<i64> = map.values().map(|e| e.used_ms).collect();
                ages.sort_unstable();
                let cutoff = ages[(ages.len() / 10).min(ages.len() - 1)];
                map.retain(|_, e| e.used_ms > cutoff);
            }
        }
        map.insert(
            session.into(),
            Entry {
                account_model,
                used_ms: now_ms,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_until_ttl_and_evicts_when_full() {
        let s = SessionAffinity::new(10, 1000);
        s.put("a", Arc::from("m1"), 0);
        assert_eq!(s.get("a", 1000).as_deref(), Some("m1"));
        assert_eq!(s.get("a", 1001), None);
        s.put("a", Arc::from("m2"), 500);
        assert_eq!(s.get("a", 1000).as_deref(), Some("m2"));

        for i in 0..20 {
            s.put(&format!("s{i}"), Arc::from("m"), 600 + i);
        }
        assert!(s.len() <= 10);
        assert!(s.get("s19", 700).is_some(), "newest kept");
        s.put(&"x".repeat(MAX_SESSION_ID_LEN + 1), Arc::from("m"), 0);
        assert!(s.get(&"x".repeat(MAX_SESSION_ID_LEN + 1), 0).is_none());
    }
}
