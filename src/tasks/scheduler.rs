use anyhow::Result;
use tokio::sync::watch;
use tokio::time::{interval, sleep, Duration};

use crate::core::state::AppState;
use crate::services::ai_grading::AiGradingService;
use crate::services::datalab_ocr::DatalabOcrService;
use crate::tasks::grading;

const OCR_WORKER_CONCURRENCY: usize = 3;
const LLM_WORKER_CONCURRENCY: usize = 3;

pub(crate) async fn run(state: AppState) -> Result<()> {
    let ai = AiGradingService::from_settings(state.settings())?;
    let datalab = DatalabOcrService::from_settings(state.settings())?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mut handles = Vec::with_capacity(OCR_WORKER_CONCURRENCY + LLM_WORKER_CONCURRENCY + 3);

    for _ in 0..OCR_WORKER_CONCURRENCY {
        handles.push(tokio::spawn(ocr_worker(state.clone(), datalab.clone(), shutdown_rx.clone())));
    }
    for _ in 0..LLM_WORKER_CONCURRENCY {
        handles.push(tokio::spawn(llm_worker(state.clone(), ai.clone(), shutdown_rx.clone())));
    }

    handles.push(tokio::spawn(process_completed_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(close_expired_loop(state.clone(), shutdown_rx.clone())));
    handles.push(tokio::spawn(retry_failed_ocr_loop(state.clone(), shutdown_rx.clone())));

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

async fn ocr_worker(
    state: AppState,
    datalab: DatalabOcrService,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }

        match grading::claim_next_ocr_submission(state.db()).await {
            Ok(Some((submission_id, course_id))) => {
                if let Err(err) =
                    grading::process_submission_ocr(&state, &datalab, &course_id, &submission_id)
                        .await
                {
                    if let Err(recovery_err) = grading::recover_ocr_submission_on_unexpected_error(
                        &state,
                        &course_id,
                        &submission_id,
                        &err.to_string(),
                    )
                    .await
                    {
                        tracing::error!(
                            course_id,
                            submission_id,
                            error = %recovery_err,
                            "Failed to recover OCR submission after worker error"
                        );
                    }
                    tracing::error!(
                        course_id,
                        submission_id,
                        error = %err,
                        "Failed to process OCR submission"
                    );
                }
                continue;
            }
            Ok(None) => {}
            Err(err) => tracing::error!(error = %err, "Failed to claim OCR submission"),
        }

        tokio::select! {
            _ = shutdown.changed() => break,
            _ = sleep(Duration::from_secs(2)) => {}
        }
    }
}

async fn llm_worker(state: AppState, ai: AiGradingService, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }

        match grading::claim_next_llm_submission(state.db()).await {
            Ok(Some((submission_id, course_id))) => {
                if let Err(err) =
                    grading::run_llm_precheck(&state, &ai, &course_id, &submission_id).await
                {
                    if let Err(recovery_err) = grading::recover_llm_submission_on_unexpected_error(
                        &state,
                        &course_id,
                        &submission_id,
                        &err.to_string(),
                    )
                    .await
                    {
                        tracing::error!(
                            course_id,
                            submission_id,
                            error = %recovery_err,
                            "Failed to recover LLM submission after worker error"
                        );
                    }
                    tracing::error!(
                        course_id,
                        submission_id,
                        error = %err,
                        "Failed to run LLM precheck"
                    );
                }
                continue;
            }
            Ok(None) => {}
            Err(err) => tracing::error!(error = %err, "Failed to claim LLM-precheck submission"),
        }

        tokio::select! {
            _ = shutdown.changed() => break,
            _ = sleep(Duration::from_secs(3)) => {}
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

async fn retry_failed_ocr_loop(state: AppState, mut shutdown: watch::Receiver<bool>) {
    let mut tick = interval(Duration::from_secs(900));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tick.tick() => {
                if let Err(err) = grading::recover_stale_processing_submissions(&state).await {
                    tracing::error!(error = %err, "recover_stale_processing_submissions failed");
                }
                if let Err(err) = grading::retry_failed_ocr_submissions(&state).await {
                    tracing::error!(error = %err, "retry_failed_ocr_submissions failed");
                }
            }
        }
    }
}
