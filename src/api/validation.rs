use crate::api::errors::ApiError;
use std::path::Path;

pub(crate) const MIN_PASSWORD_LEN: usize = 8;

pub(crate) fn validate_username(username: &str) -> Result<(), ApiError> {
    let trimmed = username.trim();
    if trimmed.len() < 3 || trimmed.len() > 64 {
        return Err(ApiError::BadRequest(
            "Username must contain from 3 to 64 characters".to_string(),
        ));
    }

    let valid = trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));

    if valid {
        Ok(())
    } else {
        Err(ApiError::BadRequest("Username contains unsupported characters".to_string()))
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

pub(crate) fn validate_image_bytes(content_type: &str, bytes: &[u8]) -> Result<(), ApiError> {
    let detected = image::guess_format(bytes)
        .map_err(|_| ApiError::BadRequest("File content is not a supported image".to_string()))?;
    let detected_mime = match detected {
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Gif => "image/gif",
        _ => {
            return Err(ApiError::BadRequest(
                "File content is not an allowed image format".to_string(),
            ))
        }
    };
    let claimed = content_type.trim().to_ascii_lowercase();
    let claimed = if claimed == "image/jpg" { "image/jpeg" } else { claimed.as_str() };
    if claimed != detected_mime {
        return Err(ApiError::BadRequest(format!(
            "MIME type '{claimed}' does not match detected file type '{detected_mime}'"
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::validate_image_bytes;

    #[test]
    fn image_bytes_must_match_claimed_mime() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert!(validate_image_bytes("image/png", png).is_ok());
        assert!(validate_image_bytes("image/jpeg", png).is_err());
        assert!(validate_image_bytes("image/png", b"not an image").is_err());
    }
}
