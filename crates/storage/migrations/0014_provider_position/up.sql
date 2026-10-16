-- SPDX-License-Identifier: Apache-2.0
-- Migration 0014: display order of providers on the accounts page (ExecPlan 023). Display only:
-- routing order comes from latency tiers and account positions (ADR-003). Existing providers
-- keep their alphabetical order.

ALTER TABLE providers ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
UPDATE providers SET position = (SELECT COUNT(*) FROM providers p2 WHERE p2.slug < providers.slug);
