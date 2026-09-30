use reqwest::Response;
use std::time::{Duration, SystemTime};

pub(super) struct Failure {
    pub message: String,
    pub retryable: bool,
    pub delay: Option<Duration>,
    pub rate_limited: bool,
}

impl Failure {
    pub(super) fn permanent(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
            delay: None,
            rate_limited: false,
        }
    }

    /// A response that fails validation carries no usable answer, so another attempt may return a valid one.
    pub(super) fn invalid(message: impl Into<String>) -> Self {
        Self {
            retryable: true,
            ..Self::permanent(message)
        }
    }

    pub(super) fn transport(error: reqwest::Error) -> Self {
        let retryable =
            error.is_timeout() || error.is_connect() || error.is_body() || error.is_request();
        Self {
            message: format!("Jev transport failed: {}", error.without_url()),
            retryable,
            delay: None,
            rate_limited: false,
        }
    }

    pub(super) fn status(response: &Response) -> Self {
        let status = response.status().as_u16();
        let mut message = format!("Jev returned HTTP {status}; no assessment was recorded");
        for name in ["x-request-id", "cf-ray"] {
            if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok())
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                message.push_str(&format!("; {name}={value}"));
            }
        }
        let delay = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|value| {
                value
                    .parse::<u64>()
                    .ok()
                    .map(Duration::from_secs)
                    .or_else(|| {
                        httpdate::parse_http_date(value)
                            .ok()
                            .map(|at| at.duration_since(SystemTime::now()).unwrap_or_default())
                    })
            });
        Self {
            message,
            retryable: matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 520 | 522 | 524),
            delay,
            rate_limited: status == 429,
        }
    }
}
