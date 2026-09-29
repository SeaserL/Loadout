// SPDX-License-Identifier: Apache-2.0

//! Built-in Provider modules (ADR-005, ADR-007: compiled in, no dynamic plugins).
//!
//! Provider-specific behaviour stays inside its module; shared code only sees
//! [`ProviderModule`] and the descriptor.

mod anthropic_compatible;
mod openai_compatible;
mod registry;

pub use anthropic_compatible::AnthropicCompatible;
pub use openai_compatible::OpenAiCompatible;
pub use registry::Registry;

#[cfg(test)]
pub(crate) mod testutil;

pub use loadout_provider_sdk::ProviderModule;
