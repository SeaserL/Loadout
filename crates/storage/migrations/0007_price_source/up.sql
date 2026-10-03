-- SPDX-License-Identifier: Apache-2.0
-- Migration 0007: where a model's prices came from (ExecPlan 011). 'user' prices are never
-- overwritten by a catalog sync; 'catalog' prices are refreshed whenever the catalog
-- reports prices. NULL = no prices recorded.

ALTER TABLE account_models ADD COLUMN price_source TEXT
    CHECK (price_source IS NULL OR price_source IN ('user', 'catalog'));
ALTER TABLE account_models ADD COLUMN price_updated_at INTEGER;

-- Existing prices: manual models were priced by the user; prices on catalog models are
-- treated as catalog prices (only catalogs that report prices can replace them).
UPDATE account_models
   SET price_source = CASE WHEN source = 'manual' THEN 'user' ELSE 'catalog' END,
       price_updated_at = updated_at
 WHERE input_price IS NOT NULL OR output_price IS NOT NULL
    OR cache_read_price IS NOT NULL OR cache_write_price IS NOT NULL;
