// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;

use loadout_provider_sdk::ProviderModule;

use crate::{
    Anthropic, AnthropicCompatible, CommandCode, DeepSeek, KimiCode, Ollama, OpenAi,
    OpenAiCompatible, OpenCodeGo, OpenRouter,
};

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
        r.register(Arc::new(OpenCodeGo));
        r.register(Arc::new(CommandCode));
        r.register(Arc::new(KimiCode));
        r.register(Arc::new(OpenRouter));
        r.register(Arc::new(DeepSeek));
        r.register(Arc::new(OpenAi));
        r.register(Arc::new(Anthropic));
        r.register(Arc::new(Ollama));
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
        assert_eq!(
            r.by_fixed_slug("opencode-go").map(|m| m.descriptor().kind),
            Some("opencode-go")
        );
        let mut slugs = std::collections::HashSet::new();
        for m in r.modules() {
            if let Some(s) = m.descriptor().fixed_slug {
                assert!(slugs.insert(s), "duplicate fixed slug {s}");
            }
            for t in m.descriptor().transports {
                assert!(
                    t.trust.allowed_for_inference(),
                    "{} transport must be tier A (ADR-009)",
                    m.descriptor().kind
                );
            }
        }
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
