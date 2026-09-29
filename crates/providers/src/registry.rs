// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;

use loadout_provider_sdk::ProviderModule;

use crate::{AnthropicCompatible, OpenAiCompatible};

/// All compiled-in provider modules, keyed by `kind`.
#[derive(Clone)]
pub struct Registry {
    modules: BTreeMap<&'static str, Arc<dyn ProviderModule>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.modules.keys()).finish()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Registry {
    pub fn builtin() -> Self {
        let mut r = Self {
            modules: BTreeMap::new(),
        };
        r.register(Arc::new(OpenAiCompatible));
        r.register(Arc::new(AnthropicCompatible));
        r
    }

    pub fn register(&mut self, module: Arc<dyn ProviderModule>) {
        self.modules.insert(module.descriptor().kind, module);
    }

    pub fn get(&self, kind: &str) -> Option<&Arc<dyn ProviderModule>> {
        self.modules.get(kind)
    }

    pub fn modules(&self) -> impl Iterator<Item = &Arc<dyn ProviderModule>> {
        self.modules.values()
    }

    /// Module that owns a fixed slug (built-in providers such as `opencode-go`).
    pub fn by_fixed_slug(&self, slug: &str) -> Option<&Arc<dyn ProviderModule>> {
        self.modules
            .values()
            .find(|m| m.descriptor().fixed_slug == Some(slug))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_modules_are_registered_by_kind() {
        let r = Registry::builtin();
        assert!(r.get("openai-compatible").is_some());
        assert!(r.get("anthropic-compatible").is_some());
        assert!(r.get("nope").is_none());
        for m in r.modules() {
            let d = m.descriptor();
            assert!(
                !d.transports.is_empty() || !d.drivers.transport,
                "{}",
                d.kind
            );
            assert_ne!(d.fixed_slug, Some("loadout"), "reserved slug");
        }
    }
}
