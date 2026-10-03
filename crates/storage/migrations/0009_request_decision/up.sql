-- SPDX-License-Identifier: Apache-2.0
-- Migration 0009: compact routing decision per request (ExecPlan 015). Metadata only
-- (ADR-008 §1): target names, skip reasons, failure kinds and HTTP statuses; never
-- prompts, responses or upstream error text. At most ~500 characters.

ALTER TABLE request_events ADD COLUMN decision TEXT;
