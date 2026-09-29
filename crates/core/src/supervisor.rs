// SPDX-License-Identifier: Apache-2.0

//! Task supervision (ADR-007).
//!
//! - **Critical** services (gateway, IPC) have no safe degraded mode: if one stops or panics,
//!   the whole daemon shuts down and reports the failure instead of running half-alive.
//! - **Restartable** background tasks (control-plane loops from Phase 3 on) are restarted
//!   with exponential backoff and never take down other tasks.

use std::future::Future;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Runs a critical service. Returns `Ok(())` if it ended because of shutdown, otherwise
/// cancels `shutdown` and returns `Err(name)`.
pub fn spawn_critical<F, E>(
    name: &'static str,
    shutdown: CancellationToken,
    service: F,
) -> JoinHandle<Result<(), &'static str>>
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let inner = tokio::spawn(service);
    tokio::spawn(async move {
        let outcome = inner.await;
        if shutdown.is_cancelled() && matches!(outcome, Ok(Ok(()))) {
            return Ok(());
        }
        match outcome {
            Ok(Ok(())) => tracing::error!(service = name, "critical service exited"),
            Ok(Err(e)) => tracing::error!(service = name, error = %e, "critical service failed"),
            Err(e) if e.is_panic() => tracing::error!(service = name, "critical service panicked"),
            Err(_) => tracing::error!(service = name, "critical service was cancelled"),
        }
        shutdown.cancel();
        Err(name)
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
        }
    }
}

/// Runs `make()` repeatedly until `shutdown`. A run that errors or panics is restarted after
/// a backoff delay that doubles up to `backoff.max`; a run that lasts longer than
/// `backoff.max` resets the delay.
pub fn spawn_restartable<M, F, E>(
    name: &'static str,
    shutdown: CancellationToken,
    backoff: Backoff,
    make: M,
) -> JoinHandle<()>
where
    M: Fn() -> F + Send + 'static,
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    tokio::spawn(async move {
        let mut delay = backoff.initial;
        loop {
            let started = tokio::time::Instant::now();
            let run = tokio::spawn(make());
            let abort = run.abort_handle();
            let outcome = tokio::select! {
                outcome = run => outcome,
                () = shutdown.cancelled() => {
                    abort.abort();
                    return;
                }
            };
            match outcome {
                Ok(Ok(())) => {
                    tracing::debug!(task = name, "background task finished");
                    return;
                }
                Ok(Err(e)) => tracing::warn!(task = name, error = %e, "background task failed"),
                Err(e) if e.is_panic() => tracing::error!(task = name, "background task panicked"),
                Err(_) => return,
            }
            if started.elapsed() > backoff.max {
                delay = backoff.initial;
            }
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = shutdown.cancelled() => return,
            }
            delay = (delay * 2).min(backoff.max);
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[tokio::test]
    async fn critical_failure_triggers_shutdown() {
        let shutdown = CancellationToken::new();
        let handle = spawn_critical("svc", shutdown.clone(), async { Err::<(), _>("boom") });
        assert_eq!(handle.await.expect("join"), Err("svc"));
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn critical_panic_triggers_shutdown() {
        let shutdown = CancellationToken::new();
        let handle = spawn_critical("svc", shutdown.clone(), async {
            if true {
                panic!("deliberate");
            }
            Ok::<(), String>(())
        });
        assert_eq!(handle.await.expect("join"), Err("svc"));
        assert!(shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn critical_clean_exit_after_shutdown_is_ok() {
        let shutdown = CancellationToken::new();
        let token = shutdown.clone();
        let handle = spawn_critical("svc", shutdown.clone(), async move {
            token.cancelled().await;
            Ok::<(), String>(())
        });
        shutdown.cancel();
        assert_eq!(handle.await.expect("join"), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn restartable_task_restarts_after_errors_and_panics() {
        let shutdown = CancellationToken::new();
        let runs = Arc::new(AtomicU32::new(0));
        let counter = runs.clone();
        let handle = spawn_restartable(
            "flaky",
            shutdown.clone(),
            Backoff {
                initial: Duration::from_millis(10),
                max: Duration::from_millis(80),
            },
            move || {
                let counter = counter.clone();
                async move {
                    match counter.fetch_add(1, Ordering::SeqCst) {
                        0 => Err("first".to_owned()),
                        1 => panic!("second"),
                        _ => Ok(()),
                    }
                }
            },
        );
        handle.await.expect("join");
        assert_eq!(runs.load(Ordering::SeqCst), 3);
        assert!(
            !shutdown.is_cancelled(),
            "background failures never stop the daemon"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn restartable_task_stops_on_shutdown() {
        let shutdown = CancellationToken::new();
        let handle = spawn_restartable("forever", shutdown.clone(), Backoff::default(), || async {
            std::future::pending::<()>().await;
            Ok::<(), String>(())
        });
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("stops")
            .expect("join");
    }
}
