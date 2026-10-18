// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;

use loadout_provider_sdk::ProviderModule;

use crate::{
    Anthropic, AnthropicCompatible, ClaudeCode, Codex, CodexSubscription, CommandCode, DeepSeek,
    GeminiCli, KimiCode, Ollama, OpenAi, OpenAiCompatible, OpenCodeBridge, OpenCodeGo, OpenRouter,
    Preset, WebChatGpt, WorkBuddyBridge,
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
        r.register(Arc::new(OpenCodeBridge));
        r.register(Arc::new(CommandCode));
        r.register(Arc::new(KimiCode));
        r.register(Arc::new(OpenRouter));
        r.register(Arc::new(DeepSeek));
        r.register(Arc::new(OpenAi));
        r.register(Arc::new(Anthropic));
        r.register(Arc::new(Ollama));
        // Table-driven API platforms (ExecPlan 018).
        for p in Preset::all() {
            r.register(Arc::new(p));
        }
        // Subscriptions: discovery and usage only (ADR-009).
        r.register(Arc::new(Codex));
        r.register(Arc::new(ClaudeCode));
        r.register(Arc::new(GeminiCli));
        // Opt-in subscription transport (ADR-012).
        r.register(Arc::new(CodexSubscription::default()));
        // Opt-in browser-session transport (ADR-014).
        r.register(Arc::new(WebChatGpt));
        // User-owned international desktop session (ADR-016).
        r.register(Arc::new(WorkBuddyBridge));
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
            r.by_fixed_slug("sensenova").map(|m| m.descriptor().kind),
            Some("sensenova")
        );
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
                // Lower-trust transports require per-account opt-in. ADR-016 adds a
                // local bridge key while the desktop client owns the upstream login.
                assert!(
                    t.trust.allowed_for_inference()
                        || matches!(
                            m.descriptor().auth,
                            loadout_provider_sdk::AuthMethod::OAuth
                                | loadout_provider_sdk::AuthMethod::BrowserLogin
                        )
                        || matches!(m.descriptor().kind, "workbuddy-bridge" | "opencode-bridge"),
                    "{} transport must be tier A or opt-in (ADR-009, ADR-012, ADR-014, ADR-016, ADR-017)",
                    m.descriptor().kind
                );
            }
        }
        // Browser sessions only for web-session modules, and the reverse (ADR-014).
        for m in r.modules() {
            let web = m
                .descriptor()
                .transports
                .iter()
                .any(|t| t.trust == loadout_provider_sdk::TrustLevel::WebSession);
            assert_eq!(web, m.web_site().is_some(), "{}", m.descriptor().kind);
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
