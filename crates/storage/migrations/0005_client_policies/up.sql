-- SPDX-License-Identifier: Apache-2.0
-- Migration 0005: per-client policies (ExecPlan 006 §3.1).

ALTER TABLE clients ADD COLUMN policy_json TEXT NOT NULL DEFAULT '{}';
