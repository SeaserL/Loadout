-- SPDX-License-Identifier: Apache-2.0

DROP TABLE pool_members;
DROP TABLE pools;
DROP TABLE latency_samples;
ALTER TABLE account_models DROP COLUMN tier_override;
