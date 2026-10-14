-- SPDX-License-Identifier: Apache-2.0
-- Migration 0010: browser-session accounts (ADR-014, ExecPlan 017 §9). NULL for every other
-- account. JSON such as {"mode":"persistent","conversation_url":"https://chatgpt.com/c/…",
-- "bound_by":"auto","min_interval_secs":10,"daily_soft_cap":150}. Never a secret and never
-- request content: the browser profile (and its cookies) lives on disk, outside SQLite.

ALTER TABLE accounts ADD COLUMN web_session_json TEXT;
