use serde_json::Value;

pub(crate) fn validate_identity_payload(
    rule_type: &str,
    rule_config: &Value,
    identity_payload: &Value,
) -> Result<(), String> {
    match rule_type {
        "none" => Ok(()),
        "isu_6_digits" => {
            let isu = identity_payload
                .get("isu")
                .and_then(Value::as_str)
                .ok_or_else(|| "Field 'isu' is required by course identity policy".to_string())?;
            let valid = isu.len() == 6 && isu.chars().all(|ch| ch.is_ascii_digit());
            if valid {
                Ok(())
            } else {
                Err("ISU must contain exactly 6 digits".to_string())
            }
        }
        "email_domain" => {
            let domain = rule_config
                .get("domain")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Identity policy is invalid: missing 'domain' in rule_config".to_string()
                })?;

            let email =
                identity_payload.get("email").and_then(Value::as_str).map(str::trim).ok_or_else(
                    || "Field 'email' is required by course identity policy".to_string(),
                )?;

            if email.to_ascii_lowercase().ends_with(&format!("@{}", domain.to_ascii_lowercase())) {
                Ok(())
            } else {
                Err(format!("Email must belong to domain {domain}"))
            }
        }
        "custom_text_validator" => {
            let required_substring = rule_config
                .get("required_substring")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Identity policy is invalid: missing 'required_substring' in rule_config"
                        .to_string()
                })?;

            let text = identity_payload
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "Field 'text' is required by course identity policy".to_string())?;

            if text.contains(required_substring) {
                Ok(())
            } else {
                Err("Identity payload does not satisfy custom validator".to_string())
            }
        }
        _ => Err("Unsupported identity policy rule_type".to_string()),
    }
}
