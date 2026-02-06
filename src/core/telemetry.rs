use tracing_subscriber::{fmt, EnvFilter};

use crate::core::config::Settings;

pub(crate) fn init_tracing(settings: &Settings) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(settings.telemetry().log_level.clone()));

    let builder = fmt().with_env_filter(filter).with_target(false);

    if settings.telemetry().json {
        builder
            .json()
            .with_span_events(fmt::format::FmtSpan::CLOSE)
            .try_init()
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    } else {
        builder
            .with_span_events(fmt::format::FmtSpan::CLOSE)
            .try_init()
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    Ok(())
}
