// SPDX-License-Identifier: Apache-2.0

//! Models an account's plan does not include (ExecPlan 022): when an upstream answers that
//! way, the model is turned off for that account (with the reason recorded) so routing stops
//! trying it. The user can turn it back on in the Models page.

use std::sync::Arc;

use loadout_router::account_model_key;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::control::{ControlPlane, blocking, storage};

pub(crate) const REASON_NOT_IN_PLAN: &str = "not_in_plan";
pub(crate) const QUEUE: usize = 64;

pub(crate) type UnavailableSender = mpsc::Sender<String>;

impl ControlPlane {
    /// Turns off the account model behind `target_id` (any protocol of it).
    pub(crate) async fn mark_model_unavailable(&self, target_id: &str) {
        let model_id = account_model_key(target_id).to_owned();
        let db = self.db.clone();
        let changed = blocking(move || {
            db.account_models()
                .mark_unavailable(&model_id, REASON_NOT_IN_PLAN)
                .map_err(storage)
        })
        .await;
        match changed {
            Ok(true) => {
                tracing::warn!(
                    target = target_id,
                    "model not in the account's plan; turned it off for this account"
                );
                if let Err(e) = self.rebuild_routes().await {
                    tracing::warn!(error = %e.message, "route rebuild failed");
                }
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e.message, "could not turn the model off"),
        }
    }
}

/// Applies reports from the gateway until shutdown.
pub(crate) async fn run(
    control: Arc<ControlPlane>,
    mut rx: mpsc::Receiver<String>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            next = rx.recv() => match next {
                Some(id) => control.mark_model_unavailable(&id).await,
                None => return,
            },
        }
    }
}
