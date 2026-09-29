// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::sync::Arc;

use loadout_protocol::Protocol;
use loadout_provider_sdk::{Capabilities, Endpoint, ProviderConfig, ProviderModule, TrustLevel};

use crate::TargetHealth;

/// `"{account_model_id}:{protocol}"`.
pub type TargetId = Arc<str>;

/// USD per million tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Prices {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl Prices {
    /// Estimated cost; `input_tokens` includes cached tokens (canonical usage semantics).
    pub fn cost(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Option<f64> {
        let (input, output) = (self.input?, self.output?);
        let uncached = input_tokens.saturating_sub(cache_read + cache_write) as f64;
        let read_price = self.cache_read.unwrap_or(input);
        let write_price = self.cache_write.unwrap_or(input);
        Some(
            (uncached * input
                + cache_read as f64 * read_price
                + cache_write as f64 * write_price
                + output_tokens as f64 * output)
                / 1_000_000.0,
        )
    }
}

/// Account + model + transport: the unit the router selects (ADR-003).
pub struct RouteTarget {
    pub id: TargetId,
    pub provider_slug: Arc<str>,
    pub account_id: Arc<str>,
    pub account_alias: Arc<str>,
    /// Account order within its provider (user controlled).
    pub account_position: i64,
    pub upstream_model: Arc<str>,
    pub protocol: Protocol,
    pub trust: TrustLevel,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub capabilities: Capabilities,
    pub prices: Prices,
    /// Prebuilt URL and headers (credentials included, marked sensitive).
    pub endpoint: Endpoint,
    pub module: Arc<dyn ProviderModule>,
    pub config: Arc<ProviderConfig>,
    pub health: Arc<TargetHealth>,
}

impl RouteTarget {
    /// `provider/account/model`.
    pub fn exact_name(&self) -> String {
        format!(
            "{}/{}/{}",
            self.provider_slug, self.account_alias, self.upstream_model
        )
    }
}

impl fmt::Debug for RouteTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteTarget")
            .field("id", &self.id)
            .field("name", &self.exact_name())
            .field("protocol", &self.protocol)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_accounts_for_cache() {
        let p = Prices {
            input: Some(2.0),
            output: Some(10.0),
            cache_read: Some(0.2),
            cache_write: None,
        };
        // 1M input of which 500k cached, 100k output.
        let cost = p.cost(1_000_000, 100_000, 500_000, 0).expect("cost");
        assert!((cost - (1.0 + 0.1 + 1.0)).abs() < 1e-9);
        assert_eq!(Prices::default().cost(1, 1, 0, 0), None);
    }
}
