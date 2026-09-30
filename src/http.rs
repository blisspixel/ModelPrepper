//! Blocking, bounded HTTP with explicit redirect trust and no credential handling.
use crate::{Error, Result};
use std::{io::Read, time::Duration};
use url::Url;

pub struct Http {
    agent: ureq::Agent,
    // A private loopback scope is used only by this crate's hermetic tests.
    test_origin: Option<String>,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(60)))
                .build()
                .new_agent(),
            test_origin: None,
        }
    }
}

impl Http {
    pub fn get(&self, url: &Url, limit: u64) -> Result<Vec<u8>> {
        let mut response = self.open(url, None)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::new(
                "source_status",
                format!("source returned HTTP {status}"),
            ));
        }
        let bound = limit
            .checked_add(1)
            .ok_or_else(|| Error::new("invalid_limit", "HTTP byte limit is too large"))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(bound)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::new("source_read_failed", "HTTP body was interrupted"))?;
        if bytes.len() as u64 > limit {
            return Err(Error::new(
                "metadata_too_large",
                "source exceeds metadata byte limit",
            ));
        }
        Ok(bytes)
    }

    pub(crate) fn open(
        &self,
        url: &Url,
        range: Option<(u64, u64)>,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let mut current = url.clone();
        for attempt in 0..=5 {
            validate_url(&current, self.test_origin.as_deref())?;
            let mut request = self
                .agent
                .get(current.as_str())
                .header("Accept-Encoding", "identity");
            if let Some((start, end)) = range {
                request = request.header("Range", format!("bytes={start}-{end}"));
            }
            let response = request.call().map_err(|_| {
                Error::new(
                    "source_unavailable",
                    "HTTP request failed; URL details withheld",
                )
            })?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if attempt == 5 {
                    return Err(Error::new(
                        "redirect_limit",
                        "source exceeded redirect limit",
                    ));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        Error::new("invalid_redirect", "redirect has no valid Location header")
                    })?;
                current = current
                    .join(location)
                    .map_err(|_| Error::new("invalid_redirect", "invalid redirect destination"))?;
                continue;
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v != "identity")
            {
                return Err(Error::new(
                    "unexpected_encoding",
                    "encoded responses cannot provide exact file bytes",
                ));
            }
            return Ok(response);
        }
        unreachable!("redirect loop always returns")
    }

    #[cfg(test)]
    pub(crate) fn fixture(origin: &str) -> Self {
        Self {
            test_origin: Some(origin.to_owned()),
            ..Self::default()
        }
    }
}

fn validate_url(url: &Url, test_origin: Option<&str>) -> Result<()> {
    let fixture = test_origin.is_some_and(|origin| url.origin().ascii_serialization() == origin);
    let host = url.host_str().unwrap_or_default();
    let trusted_host = host == "huggingface.co"
        || host.ends_with(".huggingface.co")
        || host == "hf.co"
        || host.ends_with(".hf.co");
    if (!fixture && (url.scheme() != "https" || !trusted_host || url.port().is_some()))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::new(
            "untrusted_url",
            "source or redirect is outside the allowed HTTPS hub origins",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
