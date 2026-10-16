-- SPDX-License-Identifier: Apache-2.0
-- Migration 0013: why Loadout turned a model off on its own (ExecPlan 022). NULL for models the
-- user turned off or that are on; `not_in_plan` when the upstream said the account's plan
-- does not include the model.

ALTER TABLE account_models ADD COLUMN disabled_reason TEXT;
