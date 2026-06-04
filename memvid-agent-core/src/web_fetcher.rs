use crate::extractor;
use crate::types::{FetchedContent, IngestionConfig};
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct WebFetcher {
    pub agent: ureq::Agent,
    pub config: IngestionConfig,
    pub last_request: Instant,
    pub min_interval: Duration,
}

impl WebFetcher {
    pub fn new(config: &IngestionConfig) -> Self {
        let interval_ms = 1000u32
            .checked_div(config.rate_limit_per_second)
            .unwrap_or(0);

        let agent = ureq::Agent::builder()
            .timeout_read(Duration::from_secs(config.timeout_seconds))
            .timeout_write(Duration::from_secs(config.timeout_seconds))
            .build()
            .unwrap_or_else(|_| ureq::Agent::new_with_defaults());

        Self {
            agent,
            config: config.clone(),
            last_request: Instant::now(),
            min_interval: Duration::from_millis(interval_ms as u64),
        }
    }

    pub fn fetch(&mut self, url: &str) -> Result<FetchedContent> {
        self.throttle();

        let response = self
            .agent
            .get(url)
            .call()
            .with_context(|| format!("Failed to fetch {}", url))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        let size: u64 = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if size > self.config.max_size_bytes {
            anyhow::bail!(
                "Content too large: {} bytes (max: {})",
                size,
                self.config.max_size_bytes
            );
        }

        let body = response
            .into_body()
            .read_to_string()
            .with_context(|| format!("Failed to read response body from {}", url))?;

        if body.len() as u64 > self.config.max_size_bytes {
            anyhow::bail!(
                "Content too large: {} bytes (max: {})",
                body.len(),
                self.config.max_size_bytes
            );
        }

        let raw_content = extractor::extract_text(&body, &content_type);
        let metadata = if content_type.contains("html") {
            extractor::extract_metadata(&body)
        } else {
            Default::default()
        };

        Ok(FetchedContent {
            url: url.to_string(),
            title: metadata.title,
            description: metadata.description,
            content: raw_content,
            content_type,
            size_bytes: body.len() as u64,
        })
    }

    pub fn fetch_and_retry(&mut self, url: &str) -> Result<FetchedContent> {
        let max_retries = self.config.max_retries;
        let backoff = Duration::from_secs(self.config.retry_backoff_seconds);

        let mut last_error = anyhow::anyhow!("No attempts made");
        for attempt in 0..=max_retries {
            match self.fetch(url) {
                Ok(content) => return Ok(content),
                Err(e) => {
                    last_error = e;
                    if attempt < max_retries {
                        std::thread::sleep(backoff * (attempt + 1));
                    }
                }
            }
        }
        Err(last_error)
    }

    pub fn throttle(&mut self) {
        if self.min_interval > Duration::ZERO {
            let elapsed = self.last_request.elapsed();
            if elapsed < self.min_interval {
                std::thread::sleep(self.min_interval - elapsed);
            }
        }
        self.last_request = Instant::now();
    }
}

static GLOBAL_RATE_LAST: AtomicU64 = AtomicU64::new(0);

pub fn global_throttle(requests_per_second: u32) {
    if requests_per_second == 0 {
        return;
    }
    let interval_ns = 1_000_000_000 / requests_per_second as u64;
    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let last = GLOBAL_RATE_LAST.load(Ordering::Relaxed);
        if now < last + interval_ns {
            let sleep_ns = (last + interval_ns).saturating_sub(now);
            std::thread::sleep(Duration::from_nanos(sleep_ns.min(1_000_000_000)));
            continue;
        }
        if GLOBAL_RATE_LAST
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_fetcher_new() {
        let config = IngestionConfig::default();
        let fetcher = WebFetcher::new(&config);
        assert_eq!(fetcher.config.timeout_seconds, 30);
        assert_eq!(fetcher.config.max_size_bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn web_fetcher_rejects_too_large() {
        let mut config = IngestionConfig::default();
        config.max_size_bytes = 10;
        let mut fetcher = WebFetcher::new(&config);

        let result = fetcher.fetch("https://httpbin.org/bytes/100");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("too large") || msg.contains("Failed to fetch"));
    }

    #[test]
    fn web_fetcher_timeout_short() {
        let mut config = IngestionConfig::default();
        config.timeout_seconds = 1;
        let mut fetcher = WebFetcher::new(&config);

        let result = fetcher.fetch("http://127.0.0.1:1/delay");
        assert!(result.is_err());
    }

    #[test]
    fn global_throttle_does_not_panic() {
        global_throttle(1000);
    }

    #[test]
    fn web_fetcher_fetch_nonexistent() {
        let config = IngestionConfig::default();
        let mut fetcher = WebFetcher::new(&config);
        let result = fetcher.fetch("https://nonexistent.invalid/page");
        assert!(result.is_err());
    }

    #[test]
    fn web_fetcher_retry_eventually_fails() {
        let config = IngestionConfig::default();
        let mut fetcher = WebFetcher::new(&config);
        let result = fetcher.fetch_and_retry("https://nonexistent.invalid/page");
        assert!(result.is_err());
    }

    #[test]
    fn web_fetcher_new_zero_rate_limit() {
        let mut config = IngestionConfig::default();
        config.rate_limit_per_second = 0;
        let fetcher = WebFetcher::new(&config);
        assert_eq!(fetcher.min_interval.as_millis(), 0);
    }

    #[test]
    fn web_fetcher_new_high_rate_limit() {
        let mut config = IngestionConfig::default();
        config.rate_limit_per_second = 10000;
        let fetcher = WebFetcher::new(&config);
        assert_eq!(fetcher.min_interval.as_millis(), 0);
    }

    #[test]
    fn global_throttle_zero() {
        global_throttle(0);
    }
}
