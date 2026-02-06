use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::core::config::Settings;

#[derive(Debug, Clone)]
pub(crate) struct StorageService {
    client: Client,
    bucket: String,
}

impl StorageService {
    pub(crate) async fn from_settings(settings: &Settings) -> anyhow::Result<Option<Self>> {
        if settings.s3().access_key.is_empty() || settings.s3().secret_key.is_empty() {
            return Ok(None);
        }

        let creds = Credentials::new(
            settings.s3().access_key.clone(),
            settings.s3().secret_key.clone(),
            None,
            None,
            "picrete-static",
        );

        let config = aws_config::defaults(BehaviorVersion::latest())
            .endpoint_url(settings.s3().endpoint.clone())
            .region(aws_config::Region::new(settings.s3().region.clone()))
            .credentials_provider(creds)
            .load()
            .await;

        let client = Client::new(&config);

        Ok(Some(Self { client, bucket: settings.s3().bucket.clone() }))
    }

    pub(crate) async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        expires_in: Duration,
    ) -> anyhow::Result<String> {
        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .presigned(PresigningConfig::expires_in(expires_in)?)
            .await?;

        Ok(presigned.uri().to_string())
    }

    pub(crate) async fn presign_get(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> anyhow::Result<String> {
        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(PresigningConfig::expires_in(expires_in)?)
            .await?;

        Ok(presigned.uri().to_string())
    }

    pub(crate) async fn upload_bytes(
        &self,
        key: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> anyhow::Result<(i64, String)> {
        let size = bytes.len() as i64;
        let hash = Sha256::digest(&bytes);
        let hash_hex = hex::encode(hash);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await?;

        Ok((size, hash_hex))
    }
}

#[cfg(test)]
mod tests {
    use super::StorageService;
    use crate::core::config::Settings;
    use crate::test_support;
    use std::time::Duration;

    #[tokio::test]
    async fn presign_put_and_get_return_urls() {
        let _guard = test_support::env_lock();
        test_support::set_test_env();
        test_support::set_test_storage_env();

        let settings = Settings::load().expect("settings");
        let storage = StorageService::from_settings(&settings)
            .await
            .expect("storage")
            .expect("storage enabled");

        let key = "submissions/test/file.png";
        let put_url = storage
            .presign_put(key, "image/png", Duration::from_secs(300))
            .await
            .expect("presign put");
        let get_url =
            storage.presign_get(key, Duration::from_secs(300)).await.expect("presign get");

        assert!(!put_url.is_empty());
        assert!(put_url.contains("file.png"));
        assert!(!get_url.is_empty());
        assert!(get_url.contains("file.png"));
    }
}
