// SPDX-License-Identifier: Apache-2.0

use loadout_protocol::Protocol;

use crate::Capabilities;

/// A model reported by a Catalog driver.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogModel {
    pub upstream_id: String,
    pub display_name: Option<String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    /// Empty means "use the provider's default protocols".
    pub protocols: Vec<Protocol>,
    pub capabilities: Capabilities,
    /// USD per million tokens.
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub cache_read_price: Option<f64>,
    pub cache_write_price: Option<f64>,
}

impl CatalogModel {
    pub fn new(upstream_id: impl Into<String>) -> Self {
        Self {
            upstream_id: upstream_id.into(),
            display_name: None,
            context_window: None,
            max_output_tokens: None,
            protocols: Vec::new(),
            capabilities: Capabilities::DEFAULT_MODEL,
            input_price: None,
            output_price: None,
            cache_read_price: None,
            cache_write_price: None,
        }
    }
}
