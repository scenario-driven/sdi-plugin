//! Thin HTTP client wrapper for the SDI daemon.
//!
//! Reads the daemon port from `<cache>/sdid.port` (XDG via [`sdi_db::Paths`]).
//! All entity subcommands route through this so we have one place that handles
//! "daemon not running" errors and the JSON contract.

use anyhow::{anyhow, Context, Result};
use sdi_db::Paths;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

pub struct Client {
    base: String,
    inner: reqwest::Client,
}

impl Client {
    pub fn from_paths(paths: &Paths) -> Result<Self> {
        let port = std::fs::read_to_string(&paths.port_file)
            .with_context(|| {
                format!(
                    "daemon port file not found at {} — is the daemon running? Try `sdi daemon start`.",
                    paths.port_file.display()
                )
            })?
            .trim()
            .to_string();
        let inner = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base: format!("http://127.0.0.1:{}", port),
            inner,
        })
    }

    pub fn from_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            inner: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.get(&url).send().await?;
        decode(resp).await
    }

    pub async fn post_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.post(&url).json(body).send().await?;
        decode(resp).await
    }

    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.post(&url).send().await?;
        decode(resp).await
    }

    pub async fn put_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.put(&url).json(body).send().await?;
        decode(resp).await
    }

    pub async fn delete_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.delete(&url).send().await?;
        decode(resp).await
    }

    pub async fn delete_with_body<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self.inner.delete(&url).json(body).send().await?;
        decode(resp).await
    }
}

async fn decode<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    let body = resp.bytes().await?;
    if !status.is_success() {
        // Try to surface the daemon's structured error.
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            if let Some(err) = v.get("error") {
                let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("ERROR");
                let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
                return Err(anyhow!("HTTP {} {}: {}", status.as_u16(), code, msg));
            }
        }
        return Err(anyhow!(
            "HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&body)
        ));
    }
    serde_json::from_slice::<T>(&body)
        .with_context(|| format!("decode response body: {}", String::from_utf8_lossy(&body)))
}
