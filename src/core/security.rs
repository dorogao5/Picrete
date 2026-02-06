#![allow(dead_code)]

use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Validation};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{Duration, OffsetDateTime};

use crate::core::config::Settings;

const ARGON2_MEMORY_KIB: u32 = 102_400;
const ARGON2_TIME: u32 = 2;
const ARGON2_PARALLELISM: u32 = 8;

#[derive(Debug, Error)]
pub(crate) enum SecurityError {
    #[error("password hashing failed")]
    Hashing,
    #[error("password verification failed")]
    Verification,
    #[error("jwt encoding failed")]
    JwtEncoding,
    #[error("jwt decoding failed")]
    JwtDecoding,
    #[error("unsupported jwt algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Claims {
    pub(crate) sub: String,
    pub(crate) exp: i64,
}

pub(crate) fn hash_password(password: &str) -> Result<String, SecurityError> {
    let salt = SaltString::generate(&mut OsRng);
    let params = argon2::Params::new(ARGON2_MEMORY_KIB, ARGON2_TIME, ARGON2_PARALLELISM, None)
        .map_err(|_| SecurityError::Hashing)?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| SecurityError::Hashing)?
        .to_string();

    Ok(hash)
}

pub(crate) fn verify_password(password: &str, hash: &str) -> Result<bool, SecurityError> {
    let parsed = PasswordHash::new(hash).map_err(|_| SecurityError::Verification)?;
    let params = argon2::Params::new(ARGON2_MEMORY_KIB, ARGON2_TIME, ARGON2_PARALLELISM, None)
        .map_err(|_| SecurityError::Verification)?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    match argon2.verify_password(password.as_bytes(), &parsed) {
        Ok(_) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(_) => Err(SecurityError::Verification),
    }
}

pub(crate) fn create_access_token(
    subject: &str,
    settings: &Settings,
    expires_in: Option<Duration>,
) -> Result<String, SecurityError> {
    let algorithm = algorithm_from_settings(settings)?;
    let expire = OffsetDateTime::now_utc()
        + expires_in.unwrap_or_else(|| {
            Duration::minutes(settings.security().access_token_expire_minutes as i64)
        });

    let claims = Claims { sub: subject.to_string(), exp: expire.unix_timestamp() };

    encode(
        &jsonwebtoken::Header::new(algorithm),
        &claims,
        &EncodingKey::from_secret(settings.security().secret_key.as_bytes()),
    )
    .map_err(|_| SecurityError::JwtEncoding)
}

pub(crate) fn verify_token(token: &str, settings: &Settings) -> Result<Claims, SecurityError> {
    let algorithm = algorithm_from_settings(settings)?;
    let mut validation = Validation::new(algorithm);
    validation.validate_exp = true;
    validation.required_spec_claims.insert("exp".to_string());
    validation.required_spec_claims.insert("sub".to_string());

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(settings.security().secret_key.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|_| SecurityError::JwtDecoding)
}

fn algorithm_from_settings(settings: &Settings) -> Result<Algorithm, SecurityError> {
    match settings.security().algorithm.as_str() {
        "HS256" => Ok(Algorithm::HS256),
        other => Err(SecurityError::UnsupportedAlgorithm(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("correct-horse-battery-staple").expect("hash");
        assert!(verify_password("correct-horse-battery-staple", &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn jwt_encode_decode_roundtrip() {
        std::env::set_var("SECRET_KEY", "test-secret");
        let settings = Settings::load().expect("settings");

        let token =
            create_access_token("user-123", &settings, Some(Duration::minutes(1))).expect("token");
        let claims = verify_token(&token, &settings).expect("claims");

        assert_eq!(claims.sub, "user-123");
    }
}
