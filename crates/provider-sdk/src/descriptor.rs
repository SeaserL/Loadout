// SPDX-License-Identifier: Apache-2.0

use loadout_protocol::Protocol;

use crate::{Capabilities, TrustLevel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMethod {
    /// The user supplies an API key (stored in Credential Manager).
    ApiKey,
    /// An API key is optional (e.g. local servers).
    OptionalApiKey,
    /// Credentials are managed by an official CLI; Loadout only reads them (ADR-009).
    ExternalManaged,
    /// Loadout runs its own OAuth login and owns the tokens (ADR-012).
    OAuth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportSpec {
    pub protocol: Protocol,
    pub trust: TrustLevel,
}

/// Which drivers a module implements (ADR-005 §1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drivers {
    pub discovery: bool,
    pub auth: bool,
    pub usage: bool,
    pub catalog: bool,
    pub transport: bool,
    pub health: bool,
}

/// Static description of a Provider module.
#[derive(Debug)]
pub struct ProviderDescriptor {
    /// Module id stored in `providers.kind`, e.g. `openai-compatible`.
    pub kind: &'static str,
    pub display_name: &'static str,
    /// Built-in providers have a fixed slug; generic ones take the user's.
    pub fixed_slug: Option<&'static str>,
    pub default_base_url: Option<&'static str>,
    pub auth: AuthMethod,
    /// Protocols this provider can serve, with the trust level of each transport.
    pub transports: &'static [TransportSpec],
    pub drivers: Drivers,
    pub capabilities: Capabilities,
    /// Protocols assumed for models discovered without explicit protocol information.
    pub default_model_protocols: &'static [Protocol],
}

impl ProviderDescriptor {
    pub fn transport(&self, protocol: Protocol) -> Option<&TransportSpec> {
        self.transports.iter().find(|t| t.protocol == protocol)
    }
}
