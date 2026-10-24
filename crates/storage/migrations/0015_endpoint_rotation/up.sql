-- SPDX-License-Identifier: Apache-2.0
-- Migration 0015: round-robin across the members of a dedicated endpoint (ADR-015 §5, ExecPlan 035).
-- Off by default: existing endpoints keep their sticky account order.

ALTER TABLE endpoints ADD COLUMN rotate INTEGER NOT NULL DEFAULT 0 CHECK (rotate IN (0, 1));
