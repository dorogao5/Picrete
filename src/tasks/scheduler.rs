use anyhow::Result;
use time::{OffsetDateTime, Time};
use tokio::sync::watch;
use tokio::time::{interval, sleep, Duration};

use crate::core::state::AppState;
use crate::services::ai_grading::AiGradingService;
use crate::tasks::grading;

pub(crate) async fn run(state: AppState) -> Result<()> {
    let ai = AiGradingService::from_settings(state.settings())?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mut handles = Vec::new();
    handles.push(tokio::spawn(grading_worker(state.clone(), ai.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(process_completed_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(close_expired_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(retry_failed_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(cleanup_loop(shutdown_rx.clone())));

    crate::core::shutdown::shutdown_signal().await;
    let _ = shutdown_tx.send(true);

    for handle in handles {
        let _ = handle.await;
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
            Ok(Some(submission_id)) => {
                if let Err(err) = grading::grade_submission(&state, &ai, &submission_id).await {
                    tracing::error!(submission_id, error = %err, "Failed to grade submission");
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

async fn cleanup_loop(mut shutdown: watch::Receiver<bool>) {
    loop {
        let sleep_for = duration_until(Time::from_hms(3, 0, 0).unwrap_or_else(|_| Time::MIDNIGHT));
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = sleep(sleep_for) => {
                if let Err(err) = grading::cleanup_old_results().await {
                    tracing::error!(error = %err, "cleanup_old_results failed");
                }
            }
        }
    }
}

fn duration_until(target: Time) -> Duration {
    let now = OffsetDateTime::now_utc();
    let today = now.date();
    let mut next = OffsetDateTime::new_utc(today, target);
    if next <= now {
        next += time::Duration::days(1);
    }
    let delta = next - now;
    Duration::from_secs(delta.whole_seconds().max(0) as u64)
}
