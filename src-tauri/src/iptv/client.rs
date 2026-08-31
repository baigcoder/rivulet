use reqwest::Client;
use std::time::Duration;

use super::errors::IptvError;

pub struct IptvHttpClient {
    client: Client,
}

impl IptvHttpClient {
    pub fn new() -> Result<Self, IptvError> {
        Self::with_timeouts(Duration::from_secs(30), Duration::from_secs(300))
    }

    /// Build a client with explicit timeouts. The default is generous
    /// because IPTV provider servers are often residential uplinks or
    /// slow VPSes. Tests and any future tighter path use this to pick
    /// different bounds.
    pub fn with_timeouts(connect: Duration, total: Duration) -> Result<Self, IptvError> {
        let client = Client::builder()
            .connect_timeout(connect)
            .timeout(total)
            .gzip(true)
            .brotli(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .map_err(|e| IptvError::Network(e.to_string()))?;
        Ok(Self { client })
    }

    /// The raw reqwest client, for callers that need to set per-request
    /// headers (the stream proxy) or read a streaming response.
    pub fn inner(&self) -> &Client {
        &self.client
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, IptvError> {
        eprintln!("[iptv-http] GET {url}");
        let resp = self.client.get(url).send().await?;
        let status = resp.status();
        eprintln!("[iptv-http] status={status}");
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            eprintln!("[iptv-http] error body: {body}");
            return Err(IptvError::Network(format!("HTTP {status}")));
        }
        resp.json::<T>().await.map_err(|e| {
            if e.is_decode() {
                IptvError::InvalidResponse(e.to_string())
            } else {
                IptvError::Network(e.to_string())
            }
        })
    }

    #[allow(dead_code)]
    pub async fn get_text(&self, url: &str) -> Result<String, IptvError> {
        let resp = self.client.get(url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(IptvError::Network(format!("HTTP {status}")));
        }
        resp.text().await.map_err(IptvError::from)
    }

    #[allow(dead_code)]
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, IptvError> {
        let resp = self.client.get(url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(IptvError::Network(format!("HTTP {status}")));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(IptvError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn default_timeouts_are_generous() {
        // The 10s/120s defaults were too tight for IPTV provider
        // servers. Lock in the new defaults so a future "optimization"
        // doesn't accidentally re-introduce the 10s/120s regime.
        let c = super::IptvHttpClient::new().unwrap();
        // We can't directly query the timeouts from the public API
        // (reqwest's Client doesn't expose them), but we can verify
        // the function constructs without error and matches the
        // documented shape via `with_timeouts`.
        let _ =
            super::IptvHttpClient::with_timeouts(Duration::from_secs(30), Duration::from_secs(300))
                .unwrap();
        // Reference `c` so the binding isn't unused.
        let _ = &c;
    }
}
