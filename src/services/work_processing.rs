use anyhow::{anyhow, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkProcessingSettings {
    pub(crate) ocr_enabled: bool,
    pub(crate) llm_precheck_enabled: bool,
}

impl Default for WorkProcessingSettings {
    fn default() -> Self {
        Self { ocr_enabled: true, llm_precheck_enabled: true }
    }
}

impl WorkProcessingSettings {
    pub(crate) fn validate(self) -> Result<Self> {
        if !self.ocr_enabled && self.llm_precheck_enabled {
            return Err(anyhow!(
                "Invalid processing settings: llm_precheck_enabled cannot be true when ocr_enabled is false"
            ));
        }
        Ok(self)
    }

    pub(crate) fn from_exam_settings(settings: &Value) -> Self {
        let processing = settings.get("processing").unwrap_or(settings);
        let ocr_enabled = processing.get("ocr_enabled").and_then(Value::as_bool).unwrap_or(true);
        let llm_precheck_enabled =
            processing.get("llm_precheck_enabled").and_then(Value::as_bool).unwrap_or(true);

        if !ocr_enabled {
            return Self { ocr_enabled, llm_precheck_enabled: false };
        }

        Self { ocr_enabled, llm_precheck_enabled }
    }

    pub(crate) fn merge_into_exam_settings(self, base_settings: Value) -> Value {
        let mut root = match base_settings {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        root.insert(
            "processing".to_string(),
            json!({
                "ocr_enabled": self.ocr_enabled,
                "llm_precheck_enabled": self.llm_precheck_enabled,
            }),
        );

        Value::Object(root)
    }
}
