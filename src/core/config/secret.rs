use std::{fs, path::PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;

pub(super) fn load_or_create_secret_key() -> String {
    let path = secret_file_path();

    if let Ok(value) = fs::read_to_string(&path) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let new_key = generate_secret_key();

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(
                error = %err,
                path = %parent.display(),
                "Failed to create secret key directory"
            );
        }
    }

    match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                if let Err(err) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                    tracing::warn!(
                        error = %err,
                        path = %path.display(),
                        "Failed to set secret key file permissions"
                    );
                }
            }

            if let Err(err) = std::io::Write::write_all(&mut file, new_key.as_bytes()) {
                tracing::warn!(
                    error = %err,
                    path = %path.display(),
                    "Failed to write secret key file"
                );
            }
            return new_key;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if let Ok(value) = fs::read_to_string(&path) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "Failed to create secret key file"
            );
        }
    }

    new_key
}

fn generate_secret_key() -> String {
    let mut bytes = [0u8; 64];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn secret_file_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".secret_key")
}
