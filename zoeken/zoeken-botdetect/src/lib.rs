//! Framework-free bot detection and rate limiting core.
//!
//! No web-framework dependency: callers extract [`RequestFeatures`] from
//! their own request type and call [`Detector::evaluate`]. The Axum/tower
//! adapter (extracting features from `http::Request`, wiring up a
//! `tower::Layer`) lives in `zoeken-server`.

pub mod client_ip;
pub mod config;
pub mod heuristics;
pub mod ip_lists;
pub mod link_token;
pub mod token_bucket;

use std::net::IpAddr;
use std::time::Duration;

pub use config::{ConfigError, HeaderHeuristics, LimiterConfig, RateLimitConfig};
pub use heuristics::{HeaderView, HeuristicFailure};
pub use link_token::LinkTokenVerifier;
pub use token_bucket::RateLimiter;

/// Features extracted from an inbound request.
#[derive(Debug, Clone)]
pub struct RequestFeatures {
    pub path: String,
    pub client_ip: IpAddr,
    pub headers: HeaderView,
    pub link_token: Option<String>,
}

/// The outcome of evaluating a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block(String),
    TooManyRequests(String),
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Bot detector state and evaluation logic.
#[derive(Debug)]
pub struct Detector {
    config: LimiterConfig,
    rate_limiter: RateLimiter,
    link_tokens: LinkTokenVerifier,
}

impl Detector {
    pub fn new(config: LimiterConfig, token: impl Into<String>) -> Self {
        let idle_timeout = Duration::from_secs(config.state_idle_seconds);
        Self {
            rate_limiter: RateLimiter::with_limits(config.state_capacity, idle_timeout),
            link_tokens: LinkTokenVerifier::with_limits(token, config.state_capacity, idle_timeout),
            config,
        }
    }

    pub fn config(&self) -> &LimiterConfig {
        &self.config
    }

    pub fn link_tokens(&self) -> &LinkTokenVerifier {
        &self.link_tokens
    }

    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    pub fn evaluate(&self, features: &RequestFeatures) -> Decision {
        self.evaluate_at(features, self.rate_limiter.now_secs())
    }

    /// Evaluate a request at an explicit time.
    pub fn evaluate_at(&self, features: &RequestFeatures, now: f64) -> Decision {
        if !self.config.enabled {
            return Decision::Allow;
        }

        if features.path == "/healthz" || features.path == "/readyz" {
            return Decision::Allow;
        }
        // Link-token challenge CSS must reach the handler so browsers can verify.
        if features.path.starts_with("/client") && features.path.ends_with(".css") {
            return Decision::Allow;
        }

        // Only the expensive endpoints are metered and heuristic-checked. A
        // single results page legitimately fires dozens of subresource
        // requests (favicons, proxied images, static assets); charging those
        // against the token bucket 429'd normal browsing.
        if !is_metered_path(&features.path) {
            let ip = features.client_ip;
            if ip_lists::pass_ip(ip, &self.config) {
                return Decision::Allow;
            }
            if ip_lists::block_ip(ip, &self.config) {
                return Decision::Block(format!("IP {ip} is on the block list"));
            }
            return Decision::Allow;
        }

        let ip = features.client_ip;
        let network =
            ip_lists::client_network(ip, self.config.ipv4_prefix, self.config.ipv6_prefix);
        let net_key = network.to_string();

        if ip_lists::pass_ip(ip, &self.config) {
            tracing::debug!(%network, "PASS: client IP on pass-list");
            return Decision::Allow;
        }

        if ip_lists::block_ip(ip, &self.config) {
            tracing::warn!(%network, "BLOCK: client IP on block-list");
            return Decision::Block(format!("IP {ip} is on the block list"));
        }

        let suspicious = if self.config.link_token {
            self.link_tokens
                .is_suspicious(&net_key, features.link_token.as_deref())
        } else {
            false
        };

        let link_local = ip_lists::is_link_local(ip);
        if !link_local || self.config.filter_link_local {
            let (capacity, refill) = self.config.rate_limit.params(suspicious);
            if !self.rate_limiter.check_at(&net_key, capacity, refill, now) {
                tracing::debug!(%network, suspicious, "BLOCK: rate limit exceeded");
                return Decision::TooManyRequests(format!("too many requests from {network}"));
            }
        }

        if let Err(failure) = heuristics::evaluate(&features.headers, &self.config.heuristics) {
            tracing::debug!(%network, reason = failure.reason(), "BLOCK: header heuristic");
            return Decision::TooManyRequests(failure.reason().to_string());
        }

        Decision::Allow
    }
}

/// The endpoints worth metering: upstream-fanning search and autocomplete.
fn is_metered_path(path: &str) -> bool {
    matches!(path, "/search" | "/autocompleter")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use ipnet::IpNet;

    fn browser_features(ip: &str, path: &str) -> RequestFeatures {
        RequestFeatures {
            path: path.to_string(),
            client_ip: ip.parse().unwrap(),
            headers: HeaderView {
                accept: Some("text/html".to_string()),
                accept_encoding: Some("gzip, deflate".to_string()),
                accept_language: Some("en-US".to_string()),
                connection: Some("keep-alive".to_string()),
                user_agent: Some("Mozilla/5.0 (X11; Linux x86_64) Firefox/120.0".to_string()),
                sec_fetch_mode: Some("navigate".to_string()),
                is_secure: false,
            },
            link_token: None,
        }
    }

    fn base_config() -> LimiterConfig {
        LimiterConfig {
            pass_reserved_nets: false,
            ..LimiterConfig::default()
        }
    }

    #[test]
    fn disabled_limiter_allows_everything() {
        let cfg = base_config().with_enabled(false);
        let detector = Detector::new(cfg, "tok");
        let features = RequestFeatures {
            path: "/search".to_string(),
            client_ip: "203.0.113.1".parse().unwrap(),
            headers: HeaderView::default(),
            link_token: None,
        };
        assert_eq!(detector.evaluate(&features), Decision::Allow);
    }

    #[test]
    fn healthz_is_always_allowed() {
        let detector = Detector::new(base_config(), "tok");
        let mut features = browser_features("203.0.113.1", "/healthz");
        features.headers = HeaderView::default();
        assert_eq!(detector.evaluate(&features), Decision::Allow);
    }

    #[test]
    fn readyz_is_always_allowed() {
        let detector = Detector::new(base_config(), "tok");
        let mut features = browser_features("203.0.113.1", "/readyz");
        features.headers = HeaderView::default();
        assert_eq!(detector.evaluate(&features), Decision::Allow);
    }

    /// Subresource/asset paths (favicon proxy, image proxy, static files) are
    /// not charged against the token bucket — a results page fires dozens of
    /// them and normal browsing must not 429.
    #[test]
    fn unmetered_paths_bypass_rate_limit_and_heuristics() {
        let mut cfg = base_config();
        cfg.rate_limit = RateLimitConfig {
            capacity: 1.0,
            refill_per_second: 0.0,
            suspicious_capacity: 1.0,
            suspicious_refill_per_second: 0.0,
        };
        let detector = Detector::new(cfg, "tok");
        for path in ["/favicon_proxy", "/image_proxy", "/assets/index-abc123.js"] {
            let mut features = browser_features("203.0.113.9", path);
            // Image subresources send image Accept + no-cors; must still pass.
            features.headers.accept = Some("image/avif,image/webp,*/*".to_string());
            features.headers.sec_fetch_mode = Some("no-cors".to_string());
            for _ in 0..10 {
                assert_eq!(
                    detector.evaluate_at(&features, 0.0),
                    Decision::Allow,
                    "{path} must not be metered"
                );
            }
        }
        // But blocked IPs stay blocked even on unmetered paths.
        let mut cfg = base_config();
        cfg.block_ip = vec![IpNet::from_str("198.51.100.0/24").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let features = browser_features("198.51.100.9", "/image_proxy");
        assert!(matches!(detector.evaluate(&features), Decision::Block(_)));
    }

    #[test]
    fn pass_list_bypasses_block_and_heuristics() {
        let mut cfg = base_config();
        cfg.pass_ip = vec![IpNet::from_str("203.0.113.0/24").unwrap()];
        cfg.block_ip = vec![IpNet::from_str("203.0.113.0/24").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let features = RequestFeatures {
            path: "/search".to_string(),
            client_ip: "203.0.113.5".parse().unwrap(),
            headers: HeaderView {
                user_agent: Some("curl/8.0".to_string()),
                ..HeaderView::default()
            },
            link_token: None,
        };
        assert_eq!(detector.evaluate(&features), Decision::Allow);
    }

    #[test]
    fn block_list_rejects_before_rate_limit_and_heuristics() {
        let mut cfg = base_config();
        cfg.block_ip = vec![IpNet::from_str("198.51.100.0/24").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let features = browser_features("198.51.100.9", "/search");
        assert!(matches!(detector.evaluate(&features), Decision::Block(_)));
    }

    #[test]
    fn rate_limit_rejects_after_capacity_exhausted() {
        let mut cfg = base_config();
        cfg.rate_limit = RateLimitConfig {
            capacity: 2.0,
            refill_per_second: 0.0,
            suspicious_capacity: 2.0,
            suspicious_refill_per_second: 0.0,
        };
        let detector = Detector::new(cfg, "tok");
        let features = browser_features("203.0.113.7", "/search");
        assert_eq!(detector.evaluate_at(&features, 0.0), Decision::Allow);
        assert_eq!(detector.evaluate_at(&features, 0.0), Decision::Allow);
        assert!(matches!(
            detector.evaluate_at(&features, 0.0),
            Decision::TooManyRequests(_)
        ));
    }

    #[test]
    fn header_heuristics_reject_a_bot() {
        let detector = Detector::new(base_config(), "tok");
        let mut features = browser_features("203.0.113.8", "/search");
        features.headers.user_agent = Some("curl/8.0".to_string());
        assert!(matches!(
            detector.evaluate(&features),
            Decision::TooManyRequests(_)
        ));
    }

    #[test]
    fn link_local_is_exempt_from_rate_limit_by_default() {
        let mut cfg = base_config();
        cfg.rate_limit = RateLimitConfig {
            capacity: 1.0,
            refill_per_second: 0.0,
            suspicious_capacity: 1.0,
            suspicious_refill_per_second: 0.0,
        };
        let detector = Detector::new(cfg, "tok");
        let features = browser_features("169.254.1.1", "/search");
        for _ in 0..5 {
            assert_eq!(detector.evaluate_at(&features, 0.0), Decision::Allow);
        }
    }

    #[test]
    fn suspicious_clients_get_stricter_limits_when_link_token_enabled() {
        let mut cfg = base_config();
        cfg.link_token = true;
        cfg.rate_limit = RateLimitConfig {
            capacity: 10.0,
            refill_per_second: 0.0,
            suspicious_capacity: 1.0,
            suspicious_refill_per_second: 0.0,
        };
        let detector = Detector::new(cfg, "secret");
        let features = browser_features("203.0.113.20", "/search");
        assert_eq!(detector.evaluate_at(&features, 0.0), Decision::Allow);
        assert!(matches!(
            detector.evaluate_at(&features, 0.0),
            Decision::TooManyRequests(_)
        ));
    }
}
