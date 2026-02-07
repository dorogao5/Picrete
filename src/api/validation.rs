use crate::api::errors::ApiError;
use std::path::Path;

pub(crate) const MIN_PASSWORD_LEN: usize = 8;

pub(crate) fn validate_isu(isu: &str) -> Result<(), ApiError> {
    let valid = isu.len() == 6 && isu.chars().all(|c| c.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ApiError::BadRequest("Invalid ISU format".to_string()))
    }
}

pub(crate) fn validate_password_len(password: &str) -> Result<(), ApiError> {
    if password.chars().count() >= MIN_PASSWORD_LEN {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters long"
        )))
    }
}

pub(crate) fn validate_image_upload(
    filename: &str,
    content_type: &str,
    allowed_extensions: &[String],
) -> Result<(), ApiError> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or_else(|| ApiError::BadRequest("File must have an extension".to_string()))?;

    if !allowed_extensions.iter().any(|allowed| allowed == &extension) {
        return Err(ApiError::BadRequest(format!("File extension '{extension}' is not allowed")));
    }

    let mime = content_type.trim().to_ascii_lowercase();
    if mime_allowed_for_extension(&mime, &extension) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "MIME type '{mime}' does not match extension '.{extension}'"
        )))
    }
}

fn mime_allowed_for_extension(mime: &str, extension: &str) -> bool {
    match extension {
        "jpg" | "jpeg" => matches!(mime, "image/jpeg" | "image/jpg"),
        "png" => mime == "image/png",
        "webp" => mime == "image/webp",
        "gif" => mime == "image/gif",
        _ => false,
    }
}
