//! A minimal Replit App Storage client, built on the `reqwest` already in the
//! dependency tree rather than on an SDK.
//!
//! Replit's App Storage is Google Cloud Storage with the authentication hidden
//! behind a loopback sidecar that every Replit workspace and deployment runs.
//! Their own SDKs (Python, TypeScript) are thin wrappers over the GCS client
//! libraries pointed at that sidecar; there is no Rust SDK, and the two calls
//! it takes to get a bearer token do not justify one. The contract below is
//! taken from `replit/replit-object-storage-python`
//! (`src/replit/object_storage/_config.py` and `client.py`), not guessed:
//!
//!   GET  /object-storage/default-bucket  -> {"bucketId": "..."}
//!   GET  /credential                     -> {"access_token": "<subject>"}
//!   POST /token                          -> RFC 8693 token exchange
//!
//! The last step is a standard OAuth 2.0 token exchange: the sidecar is
//! configured as a Google "identity pool" (workload identity federation)
//! provider, so the token from `/credential` is a *subject* token that must be
//! exchanged before GCS will accept it. `google.auth.identity_pool` performs
//! exactly this exchange with the parameters in `REPLIT_ADC`; `access_token`
//! below mirrors it.
//!
//! Everything here is scoped to two operations — read one object, write one
//! object — because that is all `super::snapshot` needs.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::time::Duration;

const SIDECAR_ENDPOINT: &str = "http://127.0.0.1:1106";

const GCS_ENDPOINT: &str = "https://storage.googleapis.com";

/// The sidecar is on loopback, so a slow answer means it is not there. Kept
/// short specifically so `discover` is cheap on a laptop, where nothing is
/// listening and the connection is refused immediately anyway.
const SIDECAR_TIMEOUT: Duration = Duration::from_secs(5);

/// Generous on purpose: the payload is a whole gzipped database (tens of MB),
/// not an API response.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);

/// Narrower than the `cloud-platform` scope `google-auth` defaults to. If the
/// sidecar ignores the requested scope this is inert; if it honours it, this
/// is the least privilege that still allows a snapshot to be written.
const SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_write";

#[derive(Deserialize)]
struct DefaultBucket {
    #[serde(rename = "bucketId")]
    bucket_id: Option<String>,
}

#[derive(Deserialize)]
struct SidecarCredential {
    access_token: Option<String>,
}

#[derive(Deserialize)]
struct ExchangedToken {
    access_token: Option<String>,
}

/// Cloneable so both snapshot tasks can hold one; `reqwest::Client` is
/// itself a handle to a shared connection pool, so this is cheap.
#[derive(Clone)]
pub struct ObjectStore {
    http: reqwest::Client,
    bucket: String,
    /// Where the sidecar lives, and where GCS lives. Fields rather than the
    /// constants above so the tests at the bottom of this file can point a
    /// real `ObjectStore` at a stub server and exercise the actual request
    /// shaping, auth flow and status-code handling instead of a mock of them.
    /// `discover` is the only non-test constructor and always uses the real
    /// endpoints.
    sidecar: String,
    gcs: String,
}

impl ObjectStore {
    /// Resolves the deployment's default bucket, or `Ok(None)` when there is
    /// no sidecar to ask — which is the ordinary case for `cargo run` on a
    /// laptop and must therefore be a quiet "not available", not an error.
    ///
    /// A sidecar that answers but reports no bucket *is* an error: it means
    /// App Storage has not been enabled for this app, which is a
    /// configuration mistake worth failing loudly on rather than silently
    /// running without durability.
    pub async fn discover() -> Result<Option<Self>> {
        let http = reqwest::Client::builder()
            .timeout(TRANSFER_TIMEOUT)
            .build()
            .context("could not build the HTTP client")?;

        let response = match http
            .get(format!("{SIDECAR_ENDPOINT}/object-storage/default-bucket"))
            .timeout(SIDECAR_TIMEOUT)
            .send()
            .await
        {
            Ok(r) => r,
            // Connection refused / DNS / timeout: no sidecar, so not on Replit.
            Err(_) => return Ok(None),
        };

        if !response.status().is_success() {
            bail!(
                "the Replit sidecar rejected the default-bucket request with HTTP {}",
                response.status()
            );
        }

        let bucket = response
            .json::<DefaultBucket>()
            .await
            .context("could not parse the default-bucket response")?
            .bucket_id
            .filter(|b| !b.trim().is_empty())
            .context(
                "the Replit sidecar reports no default App Storage bucket — \
                 create one from the App Storage tab, or set the bucket id in .replit",
            )?;

        Ok(Some(Self {
            http,
            bucket,
            sidecar: SIDECAR_ENDPOINT.to_string(),
            gcs: GCS_ENDPOINT.to_string(),
        }))
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Builds a store pointed at stub endpoints. Test-only; `discover` is the
    /// one way this is constructed in a running server.
    #[cfg(test)]
    pub(crate) fn with_endpoints(bucket: &str, sidecar: &str, gcs: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            bucket: bucket.to_string(),
            sidecar: sidecar.to_string(),
            gcs: gcs.to_string(),
        }
    }

    /// `Ok(None)` for a 404 — "this object does not exist yet" is a normal,
    /// expected answer on a first-ever deployment and is deliberately
    /// distinguished from a transfer that failed. `super::snapshot` relies on
    /// that distinction to decide whether writing back is safe.
    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let token = self.access_token().await?;
        let url = format!(
            "{}/storage/v1/b/{}/o/{}?alt=media",
            self.gcs,
            self.bucket,
            encode_object_name(key)
        );

        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("could not download `{key}`"))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            bail!("downloading `{key}` failed with HTTP {}", response.status());
        }

        let body = response
            .bytes()
            .await
            .with_context(|| format!("could not read the body of `{key}`"))?;
        Ok(Some(body.to_vec()))
    }

    /// Overwrites the object. GCS object writes are atomic — a reader either
    /// sees the whole previous generation or the whole new one, never a
    /// partial upload — which is what makes "snapshot straight over the live
    /// key" safe without a write-then-rename dance.
    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<()> {
        let token = self.access_token().await?;
        let url = format!(
            "{}/upload/storage/v1/b/{}/o?uploadType=media&name={}",
            self.gcs,
            self.bucket,
            encode_object_name(key)
        );

        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/gzip")
            .body(body)
            .send()
            .await
            .with_context(|| format!("could not upload `{key}`"))?;

        if !response.status().is_success() {
            bail!("uploading `{key}` failed with HTTP {}", response.status());
        }
        Ok(())
    }

    /// Fetched per operation rather than cached: a snapshot cycle makes one
    /// request every few hours, so the two extra loopback calls cost nothing
    /// and there is no expiry to track or refresh race to get wrong.
    async fn access_token(&self) -> Result<String> {
        let subject = self
            .http
            .get(format!("{}/credential", self.sidecar))
            .timeout(SIDECAR_TIMEOUT)
            .send()
            .await
            .context("could not reach the Replit credential sidecar")?
            .error_for_status()
            .context("the Replit credential sidecar returned an error")?
            .json::<SidecarCredential>()
            .await
            .context("could not parse the sidecar credential response")?
            .access_token
            .context("the sidecar credential response carried no access_token")?;

        // RFC 8693 token exchange, with the parameter values Replit's own ADC
        // configuration specifies. `subject_token_type` is the bare string
        // "access_token" rather than a URN — unusual, but it is what
        // REPLIT_ADC declares and what the sidecar expects.
        let exchanged = self
            .http
            .post(format!("{}/token", self.sidecar))
            .timeout(SIDECAR_TIMEOUT)
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("audience", "replit"),
                ("scope", SCOPE),
                (
                    "requested_token_type",
                    "urn:ietf:params:oauth:token-type:access_token",
                ),
                ("subject_token", subject.as_str()),
                ("subject_token_type", "access_token"),
            ])
            .send()
            .await
            .context("could not exchange the sidecar credential for a GCS token")?;

        if !exchanged.status().is_success() {
            bail!(
                "the Replit token exchange failed with HTTP {}",
                exchanged.status()
            );
        }

        exchanged
            .json::<ExchangedToken>()
            .await
            .context("could not parse the token-exchange response")?
            .access_token
            .context("the token-exchange response carried no access_token")
    }
}

/// Percent-encodes an object name for use as a single path segment.
///
/// GCS object names live inside one path segment of the JSON API URL, so `/`
/// — legal and common in object names — has to be escaped rather than left to
/// split the path. Hand-rolled rather than pulling in `percent-encoding`:
/// the unreserved set is four lines, and this is the only place in the
/// codebase that needs it.
fn encode_object_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode_object_name;

    #[test]
    fn leaves_unreserved_characters_alone() {
        assert_eq!(encode_object_name("mena_ai.db.gz"), "mena_ai.db.gz");
    }

    #[test]
    fn escapes_path_separators_and_spaces() {
        assert_eq!(
            encode_object_name("backups/mena ai.db"),
            "backups%2Fmena%20ai.db"
        );
    }
}
