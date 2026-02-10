use anyhow::{anyhow, Result};
use time::{Duration, PrimitiveDateTime};

use crate::db::types::WorkKind;

pub(crate) fn normalize_duration_for_kind(
    kind: WorkKind,
    duration_minutes: Option<i32>,
) -> Result<Option<i32>> {
    match kind {
        WorkKind::Control => {
            let duration = duration_minutes
                .ok_or_else(|| anyhow!("duration_minutes is required when kind is 'control'"))?;
            if duration <= 0 {
                return Err(anyhow!("duration_minutes must be positive when kind is 'control'"));
            }
            Ok(Some(duration))
        }
        WorkKind::Homework => {
            if duration_minutes.is_some() {
                return Err(anyhow!("duration_minutes must be null when kind is 'homework'"));
            }
            Ok(None)
        }
    }
}

pub(crate) fn compute_session_expiration(
    kind: WorkKind,
    session_started_at: PrimitiveDateTime,
    work_end: PrimitiveDateTime,
    duration_minutes: Option<i32>,
) -> Result<PrimitiveDateTime> {
    match kind {
        WorkKind::Control => {
            let duration = normalize_duration_for_kind(kind, duration_minutes)?
                .ok_or_else(|| anyhow!("duration_minutes is required for control works"))?;
            let duration_deadline = session_started_at + Duration::minutes(duration as i64);
            Ok(if duration_deadline < work_end { duration_deadline } else { work_end })
        }
        WorkKind::Homework => {
            normalize_duration_for_kind(kind, duration_minutes)?;
            Ok(work_end)
        }
    }
}

pub(crate) fn compute_hard_deadline(
    kind: WorkKind,
    session_started_at: PrimitiveDateTime,
    session_expires_at: PrimitiveDateTime,
    work_end: PrimitiveDateTime,
    duration_minutes: Option<i32>,
) -> Result<PrimitiveDateTime> {
    let expected_expiration =
        compute_session_expiration(kind, session_started_at, work_end, duration_minutes)?;

    Ok(if expected_expiration < session_expires_at {
        expected_expiration
    } else {
        session_expires_at
    })
}

pub(crate) fn submit_grace_period_seconds(kind: WorkKind) -> i64 {
    match kind {
        // Keep legacy control behavior for short network jitter at deadline.
        WorkKind::Control => 300,
        WorkKind::Homework => 0,
    }
}
