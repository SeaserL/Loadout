-- SPDX-License-Identifier: Apache-2.0
-- Migration 0004: latency probes, tier overrides and user pools (ExecPlan 004 §3.4).

-- User-pinned latency tier (ULTRA_FAST / FAST / NORMAL); NULL = measured.
ALTER TABLE account_models ADD COLUMN tier_override TEXT;

-- Synthetic probe results. Passive samples come from request_events.
CREATE TABLE latency_samples (
    id            INTEGER PRIMARY KEY,
    ts            INTEGER NOT NULL,
    route_target  TEXT NOT NULL,
    status        TEXT NOT NULL,
    error         TEXT,
    ttft_ms       INTEGER,
    duration_ms   INTEGER NOT NULL,
    output_tokens INTEGER,
    output_tps    REAL
) STRICT;
CREATE INDEX latency_samples_target_ts ON latency_samples(route_target, ts);

CREATE TABLE pools (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    -- manual: members in the given order; latency: ordered like loadout/default.
    ordering   TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE pool_members (
    pool_id          TEXT NOT NULL REFERENCES pools(id) ON DELETE CASCADE,
    account_model_id TEXT NOT NULL REFERENCES account_models(id) ON DELETE CASCADE,
    position         INTEGER NOT NULL,
    PRIMARY KEY (pool_id, account_model_id)
) STRICT;
