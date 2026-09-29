// SPDX-License-Identifier: Apache-2.0

//! SQLite (WAL) storage for Loadout: connection management, versioned migrations and
//! repositories. Never stores raw secrets (ADR-004).

mod clients;
mod database;
mod error;
mod migrations;
mod settings;

pub use clients::{Client, ClientId, ClientRepo, NewClient};
pub use database::{Database, now_ms};
pub use error::StorageError;
pub use migrations::{LATEST_VERSION, migrations};
pub use settings::SettingsRepo;
