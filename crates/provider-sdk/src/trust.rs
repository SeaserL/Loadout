// SPDX-License-Identifier: Apache-2.0

/// How reliable and sanctioned a transport or data source is (Harness §11, glossary §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrustLevel {
    OfficialApi,
    OfficialSubscriptionApi,
    Local,
    OfficialCli,
    CliBridge,
    PrivateEndpoint,
    WebSession,
}

impl TrustLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OfficialApi => "OFFICIAL_API",
            Self::OfficialSubscriptionApi => "OFFICIAL_SUBSCRIPTION_API",
            Self::Local => "LOCAL",
            Self::OfficialCli => "OFFICIAL_CLI",
            Self::CliBridge => "CLI_BRIDGE",
            Self::PrivateEndpoint => "PRIVATE_ENDPOINT",
            Self::WebSession => "WEB_SESSION",
        }
    }

    /// UI grouping derived from the trust level.
    pub fn tier(self) -> char {
        match self {
            Self::OfficialApi | Self::OfficialSubscriptionApi | Self::Local => 'A',
            Self::OfficialCli => 'B',
            Self::CliBridge | Self::PrivateEndpoint => 'C',
            Self::WebSession => 'D',
        }
    }

    /// v1 only routes inference through tier-A transports (ADR-009).
    pub fn allowed_for_inference(self) -> bool {
        self.tier() == 'A'
    }
}
