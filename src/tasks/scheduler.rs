use anyhow::Result;
use tokio::sync::watch;
use tokio::time::{interval, sleep, Duration};

use crate::core::state::AppState;
use crate::services::ai_grading::AiGradingService;
use crate::tasks::grading;

const GRADING_WORKER_CONCURRENCY: usize = 4;

pub(crate) async fn run(state: AppState) -> Result<()> {
    let ai = AiGradingService::from_settings(state.settings())?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mut handles = Vec::with_capacity(GRADING_WORKER_CONCURRENCY + 3);
    for _ in 0..GRADING_WORKER_CONCURRENCY {
        handles.push(tokio::spawn(grading_worker(state.clone(), ai.clone(), shutdown_rx.clone())));
    }
    handles.push(tokio::spawn(process_completed_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(close_expired_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(retry_failed_loop(state.clone(), shutdown_rx.clone())));

    crate::core::shutdown::shutdown_signal().await;
    if shutdown_tx.send(true).is_err() {
        tracing::warn!("Failed to broadcast shutdown signal to background tasks");
    }

    for handle in handles {
        if let Err(err) = handle.await {
            tracing::error!(error = %err, "Background task join failed");
        }
    }

    Ok(())
}

async fn grading_worker(
    state: AppState,
    ai: AiGradingService,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }

        match grading::claim_next_submission(state.db()).await {
            Ok(Some((submission_id, course_id))) => {
                if let Err(err) =
                    grading::grade_submission(&state, &ai, &course_id, &submission_id).await
                {
                    tracing::error!(
                        course_id,
                        submission_id,
                        error = %err,
                        "Failed to grade submission"
                    );
                }
                continue;
            }
            Ok(None) => {}
            Err(err) => tracing::error!(error = %err, "Failed to claim submission"),
        }

        tokio::select! {
            _ = shutdown.changed() => break,
            _ = sleep(Duration::from_secs(5)) => {}
        }
    }
}

async fn process_completed_loop(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut tick = interval(Duration::from_secs(300));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tick.tick() => {
                if let Err(err) = grading::process_completed_exams(&state).await {
                    tracing::error!(error = %err, "process_completed_exams failed");
                }
            }
        }
    }
}

async fn close_expired_loop(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut tick = interval(Duration::from_secs(300));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tick.tick() => {
                if let Err(err) = grading::close_expired_sessions(&state).await {
                    tracing::error!(error = %err, "close_expired_sessions failed");
                }
            }
        }
    }
}

async fn retry_failed_loop(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut tick = interval(Duration::from_secs(3600));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tick.tick() => {
                if let Err(err) = grading::retry_failed_submissions(&state).await {
                    tracing::error!(error = %err, "retry_failed_submissions failed");
                }
            }
        }
    }
}
