// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::ops::{BitOr, BitOrAssign};

/// Capability set (ADR-005 §2). Protocol support is tracked separately per model.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Capabilities(u32);

const NAMES: &[(Capabilities, &str)] = &[
    (Capabilities::STREAMING, "streaming"),
    (Capabilities::TOOLS, "tools"),
    (Capabilities::VISION, "vision"),
    (Capabilities::REASONING, "reasoning"),
    (Capabilities::RESPONSES_STATEFUL, "responses_stateful"),
    (Capabilities::USAGE_QUERY, "usage_query"),
    (Capabilities::BALANCE_QUERY, "balance_query"),
    (Capabilities::RATE_LIMIT_QUERY, "rate_limit_query"),
    (Capabilities::MODEL_DISCOVERY, "model_discovery"),
    (Capabilities::ZDR, "zdr"),
];

impl Capabilities {
    pub const NONE: Self = Self(0);
    pub const STREAMING: Self = Self(1 << 0);
    pub const TOOLS: Self = Self(1 << 1);
    pub const VISION: Self = Self(1 << 2);
    pub const REASONING: Self = Self(1 << 3);
    /// Native Responses state (`previous_response_id`, ADR-006 §3).
    pub const RESPONSES_STATEFUL: Self = Self(1 << 4);
    pub const USAGE_QUERY: Self = Self(1 << 5);
    pub const BALANCE_QUERY: Self = Self(1 << 6);
    pub const RATE_LIMIT_QUERY: Self = Self(1 << 7);
    pub const MODEL_DISCOVERY: Self = Self(1 << 8);
    pub const ZDR: Self = Self(1 << 9);

    /// What an unknown chat model is assumed to support.
    pub const DEFAULT_MODEL: Self = Self(Self::STREAMING.0 | Self::TOOLS.0);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn names(self) -> Vec<&'static str> {
        NAMES
            .iter()
            .filter(|(c, _)| self.contains(*c))
            .map(|(_, n)| *n)
            .collect()
    }

    /// Unknown names are ignored so data written by newer versions still loads.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        names
            .into_iter()
            .filter_map(|n| NAMES.iter().find(|(_, name)| *name == n).map(|(c, _)| *c))
            .fold(Self::NONE, Self::union)
    }

    pub fn to_json(self) -> String {
        serde_json::to_string(&self.names()).unwrap_or_else(|_| "[]".into())
    }

    pub fn from_json(json: &str) -> Self {
        serde_json::from_str::<Vec<String>>(json)
            .map(|v| Self::from_names(v.iter().map(String::as_str)))
            .unwrap_or_default()
    }
}

impl BitOr for Capabilities {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOrAssign for Capabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl fmt::Debug for Capabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.names()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_unknown_names() {
        let c = Capabilities::TOOLS | Capabilities::VISION;
        assert_eq!(c.to_json(), r#"["tools","vision"]"#);
        assert_eq!(Capabilities::from_json(&c.to_json()), c);
        assert_eq!(
            Capabilities::from_json(r#"["tools","from_the_future"]"#),
            Capabilities::TOOLS
        );
        assert_eq!(Capabilities::from_json("garbage"), Capabilities::NONE);
        assert!(c.contains(Capabilities::TOOLS));
        assert!(!c.contains(Capabilities::TOOLS | Capabilities::REASONING));
    }
}
