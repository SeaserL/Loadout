-- SPDX-License-Identifier: Apache-2.0
-- Migration 0001: initial schema (ExecPlan 001 §5.7).

CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE clients (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL UNIQUE,
    key_prefix TEXT NOT NULL,
    key_hash   BLOB NOT NULL UNIQUE CHECK (length(key_hash) = 32),
    enabled    INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
) STRICT;
