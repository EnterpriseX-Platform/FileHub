//! S3-compatible ObjectStore (S3, MinIO, Wasabi, R2, GCS-XML).
//!
//! Implements AWS Signature Version 4 by hand against `reqwest` — no
//! `aws-sdk-s3` because that's a multi-megabyte dependency for what is
//! ultimately four HTTP verbs.
//!
//! Configure via env:
//!   S3_ENDPOINT      = https://s3.us-east-1.amazonaws.com   (or http://minio:9000)
//!   S3_REGION        = us-east-1
//!   S3_BUCKET        = filehub-storage                        (single bucket)
//!   S3_ACCESS_KEY    = AKIA…
//!   S3_SECRET_KEY    = …
//!   S3_PATH_STYLE    = true                                    (MinIO needs it)

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use sha2::{Digest, Sha256};

use super::ObjectStore;

type HmacSha256 = Hmac<Sha256>;

pub struct S3Store {
    client:      Client,
    endpoint:    String,    // https://s3.us-east-1.amazonaws.com
    region:      String,
    bucket:      String,
    access_key:  String,
    secret_key:  String,
    path_style:  bool,
}

impl S3Store {
    pub fn from_env() -> Result<Self> {
        let endpoint   = std::env::var("S3_ENDPOINT")
            .context("S3_ENDPOINT not set (e.g. http://localhost:9000)")?;
        let region     = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into());
        let bucket     = std::env::var("S3_BUCKET").context("S3_BUCKET not set")?;
        let access_key = std::env::var("S3_ACCESS_KEY").context("S3_ACCESS_KEY not set")?;
        let secret_key = std::env::var("S3_SECRET_KEY").context("S3_SECRET_KEY not set")?;
        let path_style = std::env::var("S3_PATH_STYLE").map(|v| v != "false" && !v.is_empty()).unwrap_or(true);

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        tracing::info!("S3 store: endpoint={endpoint} bucket={bucket} region={region} path_style={path_style}");
        Ok(Self { client, endpoint, region, bucket, access_key, secret_key, path_style })
    }

    fn url_for(&self, key: &str) -> String {
        if self.path_style {
            format!("{}/{}/{}", self.endpoint.trim_end_matches('/'), self.bucket, key)
        } else {
            // virtual-hosted-style: bucket.endpoint
            // Endpoint must be a scheme://host without bucket.
            let (scheme, rest) = self.endpoint
                .split_once("://")
                .unwrap_or(("https", self.endpoint.as_str()));
            format!("{scheme}://{}.{}/{}", self.bucket, rest, key)
        }
    }

    /// Sign and dispatch a single request.  Body is required for PUT.
    async fn send(&self, method: Method, key: &str, body: Option<Bytes>) -> Result<reqwest::Response> {
        let url   = self.url_for(key);
        let parsed = reqwest::Url::parse(&url)?;
        let host = parsed.host_str().ok_or_else(|| anyhow!("invalid s3 url: {url}"))?.to_string();
        let path = parsed.path().to_string();

        let now      = Utc::now();
        let datetime = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date     = &datetime[..8];

        let body_hash = match &body {
            Some(b) => hex::encode(Sha256::digest(b)),
            None    => hex::encode(Sha256::digest(b"")),
        };

        // Headers that go into the signature, in lowercase, sorted by name.
        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), host.clone()),
            ("x-amz-content-sha256".into(), body_hash.clone()),
            ("x-amz-date".into(), datetime.clone()),
        ];
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        let signed_headers = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
        let canonical_headers: String = headers.iter()
            .map(|(k, v)| format!("{k}:{v}\n"))
            .collect();

        // SigV4 canonical request.
        let canonical_request = format!(
            "{method}\n{path}\n\n{canonical_headers}\n{signed_headers}\n{body_hash}",
            method = method.as_str(),
            path = path,
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{datetime}\n{credential_scope}\n{}",
            hex::encode(Sha256::digest(canonical_request.as_bytes()))
        );

        // Derive signing key step-by-step (AWS spec).
        fn mac(key: &[u8], msg: &str) -> Vec<u8> {
            let mut m = HmacSha256::new_from_slice(key).expect("hmac");
            m.update(msg.as_bytes());
            m.finalize().into_bytes().to_vec()
        }
        let k_date    = mac(format!("AWS4{}", self.secret_key).as_bytes(), date);
        let k_region  = mac(&k_date, &self.region);
        let k_service = mac(&k_region, "s3");
        let k_signing = mac(&k_service, "aws4_request");
        let signature = hex::encode(mac(&k_signing, &string_to_sign));

        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key,
        );

        let mut req = self.client.request(method.clone(), &url)
            .header("Host", &host)
            .header("x-amz-content-sha256", &body_hash)
            .header("x-amz-date", &datetime)
            .header("Authorization", auth);
        if let Some(b) = body {
            req = req.body(b);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("s3 {} {} → {status}: {text}", method, url);
        }
        Ok(resp)
    }
}

#[async_trait]
impl ObjectStore for S3Store {
    async fn put(&self, key: &str, body: Bytes) -> Result<()> {
        self.send(Method::PUT, key, Some(body)).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Bytes>> {
        let resp = self.send(Method::GET, key, None).await?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let bytes = resp.bytes().await?;
        Ok(Some(bytes))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.send(Method::DELETE, key, None).await?;
        Ok(())
    }

    fn label(&self) -> &str { "s3" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs a store from env if MinIO creds are present, otherwise
    /// skips. Add `MINIO_*` env in CI to flip this on.
    fn store_from_env() -> Option<S3Store> {
        if std::env::var("S3_ENDPOINT").is_err()
            || std::env::var("S3_BUCKET").is_err()
            || std::env::var("S3_ACCESS_KEY").is_err()
            || std::env::var("S3_SECRET_KEY").is_err()
        {
            return None;
        }
        S3Store::from_env().ok()
    }

    #[tokio::test]
    async fn round_trip_when_configured() {
        let Some(s) = store_from_env() else {
            eprintln!("skipping S3 round-trip: S3_* env vars not set");
            return;
        };
        super::super::tests::contract_round_trip(&s).await;
    }

    #[test]
    fn signing_key_derivation_matches_aws_example() {
        // AWS docs vector: getting started with SigV4.
        // https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
        fn mac(key: &[u8], msg: &str) -> Vec<u8> {
            let mut m = HmacSha256::new_from_slice(key).expect("hmac");
            m.update(msg.as_bytes());
            m.finalize().into_bytes().to_vec()
        }
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let k_date    = mac(format!("AWS4{secret}").as_bytes(), "20120215");
        let k_region  = mac(&k_date,    "us-east-1");
        let k_service = mac(&k_region,  "iam");
        let k_signing = mac(&k_service, "aws4_request");
        assert_eq!(
            hex::encode(&k_signing),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }
}
