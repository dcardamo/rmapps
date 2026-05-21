//! Real HttpTransport over ureq (rustls).
use super::{HttpResponse, HttpTransport};

#[derive(Debug, Default)]
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn get(&self, url: &str, token: &str) -> anyhow::Result<HttpResponse> {
        let auth = format!("Token {token}");
        let result = ureq::get(url)
            .set("Authorization", &auth)
            .timeout(std::time::Duration::from_secs(120))
            .call();
        match result {
            Ok(resp) => Ok(HttpResponse {
                status: resp.status(),
                retry_after: None,
                body: resp.into_string()?,
            }),
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp
                    .header("retry-after")
                    .and_then(|s| s.trim().parse::<u64>().ok());
                Ok(HttpResponse {
                    status: code,
                    retry_after,
                    body: resp.into_string().unwrap_or_default(),
                })
            }
            Err(e) => Err(anyhow::anyhow!("HTTP error for {url}: {e}")),
        }
    }
}
