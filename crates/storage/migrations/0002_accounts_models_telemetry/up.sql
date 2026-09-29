-- SPDX-License-Identifier: Apache-2.0
-- Migration 0002: providers, accounts, credentials, models, request telemetry
-- (ExecPlan 002 §5.2). Timestamps are Unix epoch milliseconds; prices are USD per 1M tokens.

CREATE TABLE providers (
    id           TEXT PRIMARY KEY NOT NULL,
    slug         TEXT NOT NULL UNIQUE,
    kind         TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url     TEXT,
    config_json  TEXT NOT NULL DEFAULT '{}',
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;

CREATE TABLE accounts (
    id                TEXT PRIMARY KEY NOT NULL,
    provider_id       TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    alias             TEXT NOT NULL,
    display_name      TEXT,
    account_type      TEXT NOT NULL CHECK (account_type IN ('api_key', 'subscription', 'local')),
    enabled           INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    position          INTEGER NOT NULL DEFAULT 0,
    subscription_plan TEXT,
    price_monthly_usd REAL,
    renew_at          INTEGER,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (provider_id, alias)
) STRICT;

CREATE TABLE credentials (
    id              TEXT PRIMARY KEY NOT NULL,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    credential_type TEXT NOT NULL CHECK (credential_type IN ('api_key', 'oauth', 'external_managed')),
    source          TEXT NOT NULL,
    secret_ref      TEXT,
    scope           TEXT,
    expires_at      INTEGER,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE INDEX credentials_account ON credentials(account_id);

CREATE TABLE models (
    id                TEXT PRIMARY KEY NOT NULL,
    canonical_name    TEXT NOT NULL UNIQUE,
    display_name      TEXT,
    context_window    INTEGER,
    max_output_tokens INTEGER,
    capabilities_json TEXT NOT NULL DEFAULT '[]'
) STRICT;

CREATE TABLE account_models (
    id                TEXT PRIMARY KEY NOT NULL,
    account_id        TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    upstream_model_id TEXT NOT NULL,
    model_id          TEXT REFERENCES models(id) ON DELETE SET NULL,
    protocols_json    TEXT NOT NULL DEFAULT '[]',
    context_window    INTEGER,
    max_output_tokens INTEGER,
    capabilities_json TEXT NOT NULL DEFAULT '[]',
    input_price       REAL,
    output_price      REAL,
    cache_read_price  REAL,
    cache_write_price REAL,
    enabled           INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    source            TEXT NOT NULL CHECK (source IN ('catalog', 'manual')),
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    UNIQUE (account_id, upstream_model_id)
) STRICT;

CREATE TABLE request_events (
    id                 INTEGER PRIMARY KEY,
    ts                 INTEGER NOT NULL,
    client_id          TEXT,
    requested_model    TEXT NOT NULL,
    route_target       TEXT,
    provider           TEXT,
    account            TEXT,
    upstream_model     TEXT,
    protocol_in        TEXT NOT NULL,
    protocol_out       TEXT,
    mode               TEXT,
    stream             INTEGER NOT NULL,
    status             TEXT NOT NULL,
    error_category     TEXT,
    http_status        INTEGER,
    attempts           INTEGER NOT NULL,
    input_tokens       INTEGER,
    output_tokens      INTEGER,
    cache_read_tokens  INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens   INTEGER,
    ttft_ms            INTEGER,
    duration_ms        INTEGER NOT NULL,
    output_tps         REAL,
    estimated_cost_usd REAL,
    usage_confidence   TEXT
) STRICT;

CREATE INDEX request_events_ts ON request_events(ts);
CREATE INDEX request_events_target_ts ON request_events(route_target, ts);

-- Stateful Responses requests are pinned to the target that created the response
-- (ADR-006 §3). Only ids are stored, never content.
CREATE TABLE response_routes (
    response_id  TEXT PRIMARY KEY NOT NULL,
    route_target TEXT NOT NULL,
    created_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX response_routes_created ON response_routes(created_at);
