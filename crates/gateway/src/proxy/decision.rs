// SPDX-License-Identifier: Apache-2.0

//! Compact routing decision per request (ExecPlan 015): which targets were skipped and why,
//! which attempts failed and how, and the outcome, e.g.
//! `a/b/m: skipped (quota) → c/d/m: 429 rate_limit → e/f/m: ok`.
//!
//! Metadata only (ADR-008 §1): target names, short reason codes, failure kinds and HTTP
//! statuses. Never request content, response content or upstream error text.

use std::fmt::Write as _;

use loadout_provider_sdk::UpstreamFailure;
use loadout_router::{HealthState, RouteTarget};

use crate::plane::RequestRecord;

/// Upper bound of a stored decision, in bytes.
pub(crate) const MAX_DECISION: usize = 500;
/// Room kept for the outcome step while earlier steps are appended.
const FINAL_RESERVE: usize = 160;
const SEP: &str = " → ";

#[derive(Clone, Debug, Default)]
pub(crate) struct Trail {
    text: String,
    /// Where the last step starts in `text`.
    last: usize,
    /// Steps left out because the text was already long.
    omitted: u32,
}

impl Trail {
    fn push(&mut self, step: String) {
        // Protocol variants of one account model produce identical steps; keep one.
        if !self.text.is_empty() && self.text[self.last..] == step {
            return;
        }
        if self.text.len() + step.len() > MAX_DECISION - FINAL_RESERVE {
            self.omitted += 1;
            return;
        }
        if !self.text.is_empty() {
            self.text.push_str(SEP);
        }
        self.last = self.text.len();
        self.text.push_str(&step);
    }

    /// A target that was not tried; `reason` must be a fixed, content-free phrase.
    pub fn skipped(&mut self, target: &RouteTarget, reason: &str) {
        self.push(format!("{}: skipped ({reason})", target.exact_name()));
    }

    /// A tried target that failed before anything reached the client.
    pub fn failed(&mut self, target: &RouteTarget, failure: &UpstreamFailure) {
        let mut step = format!("{}: ", target.exact_name());
        if let Some(status) = failure.status {
            let _ = write!(step, "{status} ");
        }
        step.push_str(failure.kind.code());
        self.push(step);
    }

    /// Sets the decision of a record for a request served by `target`, ending with the
    /// record's status.
    pub fn stamp(&self, mut record: RequestRecord, target: &RouteTarget) -> RequestRecord {
        record.decision = self.finish(Some((target, record.status)));
        record
    }

    /// The steps so far plus an optional outcome (`target`, record status), capped at
    /// [`MAX_DECISION`] bytes. `None` when nothing happened worth recording.
    pub fn finish(&self, outcome: Option<(&RouteTarget, &str)>) -> Option<String> {
        let mut out = self.text.clone();
        if self.omitted > 0 {
            if !out.is_empty() {
                out.push_str(SEP);
            }
            let _ = write!(out, "… {} more", self.omitted);
        }
        if let Some((target, status)) = outcome {
            if !out.is_empty() {
                out.push_str(SEP);
            }
            let _ = write!(out, "{}: {status}", target.exact_name());
        }
        if out.is_empty() {
            return None;
        }
        truncate(&mut out, MAX_DECISION);
        Some(out)
    }
}

fn truncate(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max - '…'.len_utf8();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
}

/// Short reason for a target that candidate selection excluded.
pub(crate) fn select_reason(target: &RouteTarget, why: &'static str) -> &'static str {
    match why {
        "unavailable (circuit breaker)" => match target.health.state() {
            HealthState::QuotaExhausted => "quota",
            HealthState::RateLimited => "rate limited",
            HealthState::AuthFailed => "auth failed",
            HealthState::Cooldown => "cooldown",
            HealthState::Unknown | HealthState::Healthy => "unavailable",
        },
        "transport not allowed for inference" => "not allowed",
        "input exceeds context window" => "context window",
        "model does not support tools" => "no tools",
        "stateful request needs a native Responses target" => "not native Responses",
        "stateful request is pinned to another target" => "pinned elsewhere",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_a_char_boundary() {
        let mut s = "é".repeat(400);
        truncate(&mut s, MAX_DECISION);
        assert!(s.len() <= MAX_DECISION);
        assert!(s.ends_with('…'));
        let mut short = String::from("a/b/m: ok");
        truncate(&mut short, MAX_DECISION);
        assert_eq!(short, "a/b/m: ok");
    }

    #[test]
    fn empty_trail_has_no_decision() {
        assert_eq!(Trail::default().finish(None), None);
    }

    #[test]
    fn long_trails_stay_bounded_and_count_omitted_steps() {
        let mut t = Trail::default();
        for i in 0..100 {
            t.push(format!("provider/account/model-{i}: skipped (quota)"));
        }
        let out = t.finish(None).expect("decision");
        assert!(out.len() <= MAX_DECISION);
        assert!(out.contains("more"), "{out}");
        // Duplicate consecutive steps collapse.
        let mut d = Trail::default();
        d.push("a/b/m: skipped (quota)".into());
        d.push("a/b/m: skipped (quota)".into());
        assert_eq!(d.finish(None).as_deref(), Some("a/b/m: skipped (quota)"));
    }
}
