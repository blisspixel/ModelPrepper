//! Blocking, bounded HTTP with explicit redirect trust and no credential handling.
//! A redirected location that returns 401 or 403 is abandoned. The original URL
//! is requested once more. That rejected location is not requested again, even
//! when the refreshed request redirects to it. A 401 or 403 is classified before
//! content encoding. A body that may be stored must use identity encoding,
//! matched in any letter case.
use crate::{
    Error, Result,
    buffers::{TLS_INPUT_BYTES, TLS_OUTPUT_BYTES},
};
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
                .input_buffer_size(TLS_INPUT_BYTES)
                .output_buffer_size(TLS_OUTPUT_BYTES)
                // Empty disables ureq's automatic Accept-Encoding. A ranged
                // request must not send that header. An unscoped request sets
                // identity itself.
                .accept_encoding("")
                .build()
                .new_agent(),
            test_origin: None,
        }
    }
}

impl Http {
    pub fn get(&self, url: &Url, limit: u64) -> Result<Vec<u8>> {
        let mut response = self.open(url, None).map_err(source_status_for_rejected)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::new(
                "source_status",
                format!("source returned HTTP {status}"),
            ));
        }
        read_body(&mut response, limit)
    }

    pub(crate) fn open(
        &self,
        url: &Url,
        range: Option<(u64, u64)>,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let mut current = url.clone();
        let mut redirects = 0u8;
        let mut refreshed = false;
        let mut rejected: Option<Url> = None;
        loop {
            validate_url(&current, self.test_origin.as_deref())?;
            let mut request = self.agent.get(current.as_str());
            if let Some((start, end)) = range {
                // CloudFront can answer Range plus Accept-Encoding with HTTP 200.
                // Omit the header. The caller rejects any status other than 206
                // when this range is not the whole fresh file.
                request = request.header("Range", format!("bytes={start}-{end}"));
            } else {
                request = request.header("Accept-Encoding", "identity");
            }
            let response = request.call().map_err(|_| {
                Error::new(
                    "source_unavailable",
                    "HTTP request failed; URL details withheld",
                )
            })?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if redirects == 5 {
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
                let next = current
                    .join(location)
                    .map_err(|_| Error::new("invalid_redirect", "invalid redirect destination"))?;
                if rejected.as_ref().is_some_and(|seen| seen == &next) {
                    return Err(rejected_redirect());
                }
                current = next;
                redirects += 1;
                continue;
            }
            // A rejected redirect location is not retried. One new request goes
            // back to the caller's original URL so a signed query is not reused.
            // Classify this status before content encoding so a compressed
            // error page remains a rejection.
            if matches!(status, 401 | 403) && &current != url && !refreshed {
                rejected = Some(current.clone());
                refreshed = true;
                redirects = 0;
                current = url.clone();
                continue;
            }
            if matches!(status, 401 | 403) {
                return Ok(response);
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|value| !identity_encoding(value))
            {
                return Err(Error::new(
                    "unexpected_encoding",
                    "encoded responses cannot provide exact file bytes",
                ));
            }
            return Ok(response);
        }
    }

    pub(crate) fn buffer_sizes(&self) -> (usize, usize) {
        (
            self.agent.config().input_buffer_size(),
            self.agent.config().output_buffer_size(),
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture(origin: &str) -> Self {
        Self {
            test_origin: Some(origin.to_owned()),
            ..Self::default()
        }
    }
}

pub(crate) fn read_body(
    response: &mut ureq::http::Response<ureq::Body>,
    limit: u64,
) -> Result<Vec<u8>> {
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

pub(crate) fn rejected_redirect() -> Error {
    Error::new(
        "rejected_redirect",
        "a rejected redirect location was not requested again",
    )
}

pub(crate) fn source_status_for_rejected(error: Error) -> Error {
    if error.code == "rejected_redirect" {
        Error::new(
            "source_status",
            "a rejected redirect location was not requested again",
        )
    } else {
        error
    }
}

fn identity_encoding(value: &ureq::http::HeaderValue) -> bool {
    value.to_str().is_ok_and(|text| {
        !text.is_empty()
            && text
                .split(',')
                .all(|part| part.trim().eq_ignore_ascii_case("identity"))
    })
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
