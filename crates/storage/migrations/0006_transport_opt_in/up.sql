-- SPDX-License-Identifier: Apache-2.0
-- Migration 0006: per-account opt-in for lower-trust subscription transports (ADR-012,
-- ExecPlan 010 §2.3). NULL = not opted in; otherwise the confirmation time (unix ms).

ALTER TABLE accounts ADD COLUMN transport_opt_in_at INTEGER;
