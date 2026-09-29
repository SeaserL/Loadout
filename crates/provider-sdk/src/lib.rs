// SPDX-License-Identifier: Apache-2.0

//! The contract every Provider module implements (ADR-005).
//!
//! A module declares what it is ([`ProviderDescriptor`]: drivers, capabilities, trust level of
//! each transport) and implements [`ProviderModule`] for the drivers it has. Missing drivers
//! never prevent a provider from being added.

mod capability;
mod catalog;
mod descriptor;
mod error;
mod failure;
mod http;
mod module;
mod trust;
mod url;
pub mod usage;

pub use capability::Capabilities;
pub use catalog::CatalogModel;
pub use descriptor::{AuthMethod, Drivers, ProviderDescriptor, TransportSpec};
pub use error::ProviderError;
pub use failure::{FailureKind, UpstreamFailure, classify, error_message};
pub use http::{HttpClient, HttpConfig};
pub use module::{BoxFuture, DriverContext, Endpoint, ProviderConfig, ProviderModule};
pub use trust::TrustLevel;
pub use url::{is_loopback_url, join_url, validate_base_url};
