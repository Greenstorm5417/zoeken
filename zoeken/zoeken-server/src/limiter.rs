//! Axum/tower adapter over `zoeken-botdetect`'s framework-free `Detector`.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower::{Layer, Service};
use zoeken_botdetect::{Decision, Detector, HeaderView, LimiterConfig, RequestFeatures, client_ip};

pub fn layer(detector: Arc<Detector>) -> BotDetectLayer {
    BotDetectLayer { detector }
}

/// `tower` layer that installs `BotDetectService`.
#[derive(Clone)]
pub struct BotDetectLayer {
    detector: Arc<Detector>,
}

impl BotDetectLayer {
    pub fn new(detector: Arc<Detector>) -> Self {
        Self { detector }
    }
}

impl<S> Layer<S> for BotDetectLayer {
    type Service = BotDetectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BotDetectService {
            inner,
            detector: self.detector.clone(),
        }
    }
}

/// `tower` service that evaluates each request before forwarding it.
#[derive(Clone)]
pub struct BotDetectService<S> {
    inner: S,
    detector: Arc<Detector>,
}

impl<S> Service<Request<Body>> for BotDetectService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let detector = self.detector.clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            match extract_features(&req, detector.config()) {
                None => inner.call(req).await,
                Some(features) => match detector.evaluate(&features) {
                    Decision::Allow => inner.call(req).await,
                    Decision::Block(msg) => Ok((StatusCode::FORBIDDEN, msg).into_response()),
                    Decision::TooManyRequests(msg) => {
                        Ok((StatusCode::TOO_MANY_REQUESTS, msg).into_response())
                    }
                },
            }
        })
    }
}

fn extract_features(req: &Request<Body>, config: &LimiterConfig) -> Option<RequestFeatures> {
    let headers = req.headers();

    let header_str = |name: header::HeaderName| -> Option<String> {
        headers
            .get(&name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    };
    let header_named = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    };

    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());
    let x_forwarded_for = header_named("x-forwarded-for")
        .map(|value| client_ip::parse_forwarded_for(&value))
        .unwrap_or_default();
    let x_real_ip = header_named("x-real-ip").and_then(|value| value.trim().parse::<IpAddr>().ok());

    let client_ip =
        client_ip::derive_client_ip(peer, &x_forwarded_for, x_real_ip, &config.trusted_proxies)?;

    let is_secure = match header_named("x-forwarded-proto") {
        Some(proto) => proto.eq_ignore_ascii_case("https"),
        None => req.uri().scheme_str() == Some("https"),
    };

    let view = HeaderView {
        accept: header_str(header::ACCEPT),
        accept_encoding: header_str(header::ACCEPT_ENCODING),
        accept_language: header_str(header::ACCEPT_LANGUAGE),
        connection: header_str(header::CONNECTION),
        user_agent: header_str(header::USER_AGENT),
        sec_fetch_mode: header_named("sec-fetch-mode"),
        is_secure,
    };

    let link_token = header_named("x-link-token").or_else(|| {
        req.uri()
            .query()
            .and_then(|q| url_form_value(q, "link_token"))
    });

    Some(RequestFeatures {
        path: req.uri().path().to_string(),
        client_ip,
        headers: view,
        link_token,
    })
}

fn url_form_value(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return Some(it.next().unwrap_or("").to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use axum::body::to_bytes;
    use ipnet::IpNet;
    use tower::ServiceExt;
    use zoeken_botdetect::LimiterConfig;

    fn with_peer(mut req: Request<Body>, peer: &str) -> Request<Body> {
        let addr: SocketAddr = format!("{peer}:12345").parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        req
    }

    fn base_config() -> LimiterConfig {
        LimiterConfig {
            pass_reserved_nets: false,
            ..LimiterConfig::default()
        }
    }

    async fn allow_ok(_req: Request<Body>) -> Result<Response, std::convert::Infallible> {
        Ok((StatusCode::OK, "handler reached").into_response())
    }

    fn service_with(
        detector: Detector,
    ) -> BotDetectService<
        tower::util::BoxCloneService<Request<Body>, Response, std::convert::Infallible>,
    > {
        let inner = tower::util::BoxCloneService::new(tower::service_fn(allow_ok));
        BotDetectLayer::new(Arc::new(detector)).layer(inner)
    }

    #[tokio::test]
    async fn fail_open_when_client_ip_cannot_be_determined() {
        let detector = Detector::new(base_config(), "tok");
        let service = service_with(detector);
        let req = Request::builder()
            .uri("/search?q=rust")
            .body(Body::empty())
            .unwrap();
        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"handler reached");
    }

    #[tokio::test]
    async fn block_listed_ip_via_x_real_ip_is_rejected() {
        let mut cfg = base_config();
        cfg.trusted_proxies = vec![IpNet::from_str("10.0.0.0/8").unwrap()];
        cfg.block_ip = vec![IpNet::from_str("198.51.100.0/24").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let service = service_with(detector);
        let req = with_peer(
            Request::builder()
                .uri("/search?q=rust")
                .header("x-real-ip", "198.51.100.9")
                .header(header::ACCEPT, "text/html")
                .header(header::ACCEPT_ENCODING, "gzip")
                .header(header::ACCEPT_LANGUAGE, "en")
                .header(header::USER_AGENT, "Mozilla/5.0 Firefox/120.0")
                .body(Body::empty())
                .unwrap(),
            "10.0.0.1",
        );
        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn spoofed_x_real_ip_ignored_from_untrusted_peer() {
        let mut cfg = base_config();
        cfg.trusted_proxies = vec![IpNet::from_str("10.0.0.0/8").unwrap()];
        cfg.block_ip = vec![IpNet::from_str("198.51.100.0/24").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let service = service_with(detector);
        let req = with_peer(
            Request::builder()
                .uri("/search?q=rust")
                .header("x-real-ip", "198.51.100.9")
                .header(header::ACCEPT, "text/html")
                .header(header::ACCEPT_ENCODING, "gzip, deflate")
                .header(header::ACCEPT_LANGUAGE, "en-US")
                .header(header::CONNECTION, "keep-alive")
                .header(header::USER_AGENT, "Mozilla/5.0 Firefox/120.0")
                .body(Body::empty())
                .unwrap(),
            "192.0.2.5",
        );
        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "untrusted peer must not spoof into the block list via X-Real-IP"
        );
    }

    #[tokio::test]
    async fn clean_browser_request_reaches_handler() {
        let mut cfg = base_config();
        cfg.trusted_proxies = vec![IpNet::from_str("10.0.0.0/8").unwrap()];
        let detector = Detector::new(cfg, "tok");
        let service = service_with(detector);
        let req = with_peer(
            Request::builder()
                .uri("/search?q=rust")
                .header("x-real-ip", "203.0.113.50")
                .header(header::ACCEPT, "text/html")
                .header(header::ACCEPT_ENCODING, "gzip, deflate")
                .header(header::ACCEPT_LANGUAGE, "en-US")
                .header(header::CONNECTION, "keep-alive")
                .header(header::USER_AGENT, "Mozilla/5.0 Firefox/120.0")
                .body(Body::empty())
                .unwrap(),
            "10.0.0.1",
        );
        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
