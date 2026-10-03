-- SPDX-License-Identifier: Apache-2.0
-- Migration 0008: per-account quota reserve (ADR-003 §9, ExecPlan 013). NULL = use the
-- global `routing.quota_reserve_percent`; otherwise JSON such as
-- {"default":10,"5h":10,"weekly":20} (percent remaining, keyed by window scope).

ALTER TABLE accounts ADD COLUMN quota_reserve_json TEXT;
