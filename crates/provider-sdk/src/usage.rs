// SPDX-License-Identifier: Apache-2.0

//! Usage / quota / balance model (product doc §21, §35; glossary §6).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UsageSource {
    OfficialApi,
    OfficialCli,
    LocalFiles,
    PrivateEndpoint,
    Estimated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Confidence {
    Exact,
    Estimated,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    /// `used` / `limit` are percentages (0–100).
    Percent,
    Requests,
    Tokens,
    Usd,
    Credits,
}

/// A rolling or fixed quota window, e.g. "5h", "weekly", "monthly".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    /// Stable key, e.g. `5h`, `weekly`, `monthly`, `daily`, `weekly:opus`.
    pub scope: String,
    pub label: String,
    pub unit: QuotaUnit,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub window_seconds: Option<u64>,
    /// Quota reset (distinct from subscription renewal, product doc §35).
    pub resets_at_ms: Option<i64>,
}

impl QuotaWindow {
    /// Remaining fraction in 0.0–1.0, when computable.
    pub fn remaining_fraction(&self) -> Option<f64> {
        match (self.unit, self.used, self.limit) {
            (QuotaUnit::Percent, Some(used), _) => Some(((100.0 - used) / 100.0).clamp(0.0, 1.0)),
            (_, Some(used), Some(limit)) if limit > 0.0 => {
                Some(((limit - used) / limit).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }

    pub fn is_exhausted(&self) -> bool {
        self.remaining_fraction().is_some_and(|r| r <= 0.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Balance {
    pub currency: String,
    pub amount: f64,
    /// Credit included with a subscription, if reported separately.
    pub included: Option<f64>,
    pub topup: Option<f64>,
    /// Total granted for the period, when known (e.g. `$38.27 / $70`).
    pub granted: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub name: String,
    pub price_usd_monthly: Option<f64>,
    /// Next billing / renewal date (not the quota reset).
    pub renews_at_ms: Option<i64>,
    pub auto_renew: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    pub source: UsageSource,
    pub confidence: Confidence,
    pub windows: Vec<QuotaWindow>,
    pub balances: Vec<Balance>,
    pub plan: Option<Plan>,
    /// Account identity shown in the UI (e.g. an email), never a secret.
    pub account_label: Option<String>,
}

impl UsageReport {
    pub fn new(source: UsageSource, confidence: Confidence) -> Self {
        Self {
            source,
            confidence,
            windows: Vec::new(),
            balances: Vec::new(),
            plan: None,
            account_label: None,
        }
    }

    /// True when any window or balance is fully used.
    pub fn is_exhausted(&self) -> bool {
        self.windows.iter().any(QuotaWindow::is_exhausted)
            || self
                .balances
                .iter()
                .any(|b| b.amount <= 0.0 && b.granted.is_some())
    }

    /// Earliest reset among exhausted windows.
    pub fn exhausted_until_ms(&self) -> Option<i64> {
        self.windows
            .iter()
            .filter(|w| w.is_exhausted())
            .filter_map(|w| w.resets_at_ms)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(unit: QuotaUnit, used: Option<f64>, limit: Option<f64>) -> QuotaWindow {
        QuotaWindow {
            scope: "5h".into(),
            label: "5h".into(),
            unit,
            used,
            limit,
            window_seconds: Some(18_000),
            resets_at_ms: Some(1_000),
        }
    }

    #[test]
    fn remaining_fraction() {
        assert_eq!(
            window(QuotaUnit::Percent, Some(78.0), None).remaining_fraction(),
            Some(0.22_f64.max(0.22))
        );
        assert_eq!(
            window(QuotaUnit::Usd, Some(8.3), Some(12.0))
                .remaining_fraction()
                .map(|r| (r * 100.0).round()),
            Some(31.0)
        );
        assert!(window(QuotaUnit::Percent, Some(100.0), None).is_exhausted());
        assert_eq!(
            window(QuotaUnit::Tokens, None, Some(1.0)).remaining_fraction(),
            None
        );
    }

    #[test]
    fn exhausted_until_is_earliest_reset() {
        let mut r = UsageReport::new(UsageSource::OfficialApi, Confidence::Exact);
        r.windows
            .push(window(QuotaUnit::Percent, Some(100.0), None));
        let mut weekly = window(QuotaUnit::Percent, Some(100.0), None);
        weekly.resets_at_ms = Some(500);
        r.windows.push(weekly);
        assert!(r.is_exhausted());
        assert_eq!(r.exhausted_until_ms(), Some(500));
    }
}
