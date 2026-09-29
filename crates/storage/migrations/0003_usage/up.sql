-- SPDX-License-Identifier: Apache-2.0
-- Migration 0003: latest usage per account (ExecPlan 003 §4.1).

CREATE TABLE usage_snapshots (
    account_id    TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    fetched_at    INTEGER NOT NULL,
    source        TEXT,
    confidence    TEXT,
    plan_json     TEXT,
    account_label TEXT,
    error         TEXT
) STRICT;

CREATE TABLE quota_windows (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    scope          TEXT NOT NULL,
    label          TEXT NOT NULL,
    unit           TEXT NOT NULL,
    used           REAL,
    limit_value    REAL,
    window_seconds INTEGER,
    resets_at      INTEGER,
    observed_at    INTEGER NOT NULL,
    PRIMARY KEY (account_id, scope)
) STRICT;

CREATE TABLE balances (
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    currency    TEXT NOT NULL,
    amount      REAL NOT NULL,
    included    REAL,
    topup       REAL,
    granted     REAL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, currency)
) STRICT;
