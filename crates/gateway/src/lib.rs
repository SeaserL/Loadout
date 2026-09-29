// SPDX-License-Identifier: Apache-2.0

//! The data-plane HTTP gateway bound to loopback (ADR-011).
//!
//! The request path must never touch the database or the network before route selection
//! (Harness §2.2); all lookups here are in-memory.
