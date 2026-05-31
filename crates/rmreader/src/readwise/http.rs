//! Real HttpTransport over ureq (rustls).
use super::{HttpMethod, HttpResponse, HttpTransport};

#[derive(Debug, Default)]
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn request(
        &self,
        method: HttpMethod,
        url: &str,
        token: &str,
        body: Option<&str>,
    ) -> anyhow::Result<HttpResponse> {
        let method_str = match method {
            HttpMethod::Get => "GET",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Post => "POST",
        };
        let auth = format!("Token {token}");
        let req = ureq::request(method_str, url)
            .set("Authorization", &auth)
            .timeout(std::time::Duration::from_secs(120));
        let result = if let Some(b) = body {
            req.set("Content-Type", "application/json").send_string(b)
        } else {
            req.call()
        };
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
