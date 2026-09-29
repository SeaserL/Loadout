// SPDX-License-Identifier: Apache-2.0

//! Secret wrapper types and the OS-backed secret store.
//!
//! Raw secrets never enter SQLite or logs; only a [`SecretRef`]-style handle is persisted
//! (ADR-004). The Windows implementation stores secrets in Credential Manager.
