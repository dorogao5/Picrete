use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::core::config::Settings;

#[derive(Debug, Error)]
pub(crate) enum MaterialsError {
    #[error("invalid relative path")]
    InvalidRelativePath,
    #[error("path escapes configured root")]
    PathOutsideRoot,
    #[error("file not found")]
    NotFound,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub(crate) fn task_bank_root(settings: &Settings) -> Result<PathBuf, MaterialsError> {
    canonicalize_existing(resolve_config_path(&settings.task_bank().root)?)
}

pub(crate) fn task_bank_json_path(settings: &Settings) -> Result<PathBuf, MaterialsError> {
    let root = task_bank_root(settings)?;
    canonicalize_existing(root.join("Sviridov_tasks.json"))
}

pub(crate) fn task_bank_media_root(settings: &Settings) -> Result<PathBuf, MaterialsError> {
    canonicalize_existing(resolve_config_path(&settings.task_bank().media_root)?)
}

pub(crate) fn addition_pdf_path(settings: &Settings) -> Result<PathBuf, MaterialsError> {
    let media_root = task_bank_media_root(settings)?;
    let pdf_path = canonicalize_existing(resolve_config_path(
        &settings.task_bank().additional_materials_pdf,
    )?)?;
    if !pdf_path.starts_with(&media_root) {
        return Err(MaterialsError::PathOutsideRoot);
    }
    Ok(pdf_path)
}

pub(crate) fn resolve_task_bank_media_path(
    settings: &Settings,
    raw_relative_path: &str,
) -> Result<PathBuf, MaterialsError> {
    let media_root = task_bank_media_root(settings)?;
    let relative = sanitize_relative_path(raw_relative_path)?;
    let joined = media_root.join(relative);
    let canonical = canonicalize_existing(joined)?;
    if !canonical.starts_with(&media_root) {
        return Err(MaterialsError::PathOutsideRoot);
    }
    Ok(canonical)
}

pub(crate) fn sanitize_relative_path(raw: &str) -> Result<PathBuf, MaterialsError> {
    let normalized = raw.trim().replace('\\', "/").trim_start_matches('/').to_string();
    if normalized.is_empty() {
        return Err(MaterialsError::InvalidRelativePath);
    }

    let path = Path::new(&normalized);
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MaterialsError::InvalidRelativePath);
            }
        }
    }

    Ok(path.to_path_buf())
}

pub(crate) fn normalize_sviridov_image_path(raw: &str) -> Result<String, MaterialsError> {
    const PREFIX: &str = "ocr_output/Sviridov_tasks/";

    let sanitized = sanitize_relative_path(raw)?;
    let as_text = sanitized.to_string_lossy().to_string();
    let stripped = as_text.strip_prefix(PREFIX).unwrap_or(&as_text);
    if stripped.is_empty() {
        return Err(MaterialsError::InvalidRelativePath);
    }
    Ok(stripped.to_string())
}

pub(crate) fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn resolve_config_path(value: &str) -> Result<PathBuf, MaterialsError> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn canonicalize_existing(path: PathBuf) -> Result<PathBuf, MaterialsError> {
    if !path.exists() {
        return Err(MaterialsError::NotFound);
    }
    Ok(std::fs::canonicalize(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_parent() {
        let result = sanitize_relative_path("../bad.png");
        assert!(matches!(result, Err(MaterialsError::InvalidRelativePath)));
    }

    #[test]
    fn sanitize_accepts_normal_path() {
        let result = sanitize_relative_path("page_0011/images/0.jpg").expect("valid path");
        assert_eq!(result.to_string_lossy(), "page_0011/images/0.jpg");
    }

    #[test]
    fn normalize_sviridov_prefix() {
        let result =
            normalize_sviridov_image_path("ocr_output/Sviridov_tasks/page_0011/images/0.jpg")
                .expect("normalized");
        assert_eq!(result, "page_0011/images/0.jpg");
    }
}
