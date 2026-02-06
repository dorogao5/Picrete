use std::sync::OnceLock;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::core::config::Settings;

static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

pub(crate) fn init(settings: &Settings) -> anyhow::Result<()> {
    if !settings.telemetry().prometheus_enabled {
        return Ok(());
    }

    let handle = PrometheusBuilder::new().install_recorder()?;
    let _ = PROM_HANDLE.set(handle);
    Ok(())
}

pub(crate) fn render() -> Option<String> {
    PROM_HANDLE.get().map(|handle| handle.render())
}
