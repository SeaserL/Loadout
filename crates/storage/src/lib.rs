// SPDX-License-Identifier: Apache-2.0

//! SQLite (WAL) storage for Loadout: connection management, versioned migrations,
//! repositories and the asynchronous telemetry writer. Never stores raw secrets (ADR-004)
//! or request content (ADR-008).

mod account_models;
mod accounts;
mod clients;
mod database;
mod error;
mod latency;
mod migrations;
mod providers;
mod request_events;
mod response_routes;
mod routing;
mod settings;
pub mod telemetry;
mod usage;

pub use account_models::{AccountModel, AccountModelRepo, ModelInput, SyncCounts};
pub use accounts::{Account, AccountRepo, Credential, NewAccount};
pub use clients::{Client, ClientId, ClientRepo, NewClient};
pub use database::{Database, now_ms};
pub use error::StorageError;
pub use latency::{HistorySample, Pool, ProbeSample};
pub use migrations::{LATEST_VERSION, migrations};
pub use providers::{NewProvider, Provider, ProviderRepo};
pub use request_events::RequestEvent;
pub use routing::RoutableModel;
pub use settings::SettingsRepo;
pub use usage::{BalanceRow, Spend, UsageRecord, WindowRow};

impl Database {
    pub fn providers(&self) -> ProviderRepo<'_> {
        ProviderRepo::new(self)
    }

    pub fn accounts(&self) -> AccountRepo<'_> {
        AccountRepo::new(self)
    }

    pub fn account_models(&self) -> AccountModelRepo<'_> {
        AccountModelRepo::new(self)
    }
}
