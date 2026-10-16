-- SPDX-License-Identifier: Apache-2.0
-- Migration 0011: account sub-pools (ExecPlan 019). The main pool (every account) is not
-- stored: no active set means the main pool. At most one set is active.

CREATE TABLE account_sets (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    active     INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX account_sets_one_active ON account_sets(active) WHERE active = 1;

CREATE TABLE account_set_members (
    set_id     TEXT NOT NULL REFERENCES account_sets(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    PRIMARY KEY (set_id, account_id)
) STRICT;
