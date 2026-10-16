-- SPDX-License-Identifier: Apache-2.0
-- Migration 0012: dedicated endpoints (ADR-015, ExecPlan 021). `members_json` NULL means every
-- account of the provider in the current pool; otherwise a JSON array of account ids.

CREATE TABLE endpoints (
    name          TEXT PRIMARY KEY NOT NULL,
    provider_slug TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    members_json  TEXT,
    default_model TEXT,
    wait_secs     INTEGER NOT NULL DEFAULT 30 CHECK (wait_secs BETWEEN 0 AND 300),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;
