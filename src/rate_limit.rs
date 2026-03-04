use std::net::{IpAddr, SocketAddr};

use axum::http;
use tower_governor::{
    errors::GovernorError,
    key_extractor::{KeyExtractor, PeerIpKeyExtractor, SmartIpKeyExtractor},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitIpMode {
    /// Trust only the peer IP (`ConnectInfo` / `SocketAddr` extension).
    /// This is safe for direct access (not behind a trusted proxy).
    Peer,
    /// Resolve client IP from proxy headers (`Forwarded` / `X-Forwarded-For` /
    /// `X-Real-Ip`) with peer-IP fallback. Only use behind a trusted
    /// reverse proxy.
    Smart,
}

impl RateLimitIpMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "peer" => Some(Self::Peer),
            "smart" => Some(Self::Smart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitIpKeyExtractor {
    mode: RateLimitIpMode,
}

impl RateLimitIpKeyExtractor {
    pub fn new(mode: RateLimitIpMode) -> Self { Self { mode } }

    // GovernorError is defined in an external crate and cannot be boxed
    // without breaking the KeyExtractor trait contract.
    #[allow(clippy::result_large_err)]
    fn extract_peer_ip<T>(
        &self,
        req: &http::Request<T>,
    ) -> Result<IpAddr, GovernorError> {
        // `PeerIpKeyExtractor` only checks `ConnectInfo<SocketAddr>`. We also
        // fall back to `SocketAddr` extension because it is still "peer
        // IP" and keeps behavior robust.
        PeerIpKeyExtractor.extract(req).or_else(|_| {
            if let Some(addr) = req.extensions().get::<SocketAddr>() {
                Ok(addr.ip())
            } else {
                Err(GovernorError::UnableToExtractKey)
            }
        })
    }
}

impl KeyExtractor for RateLimitIpKeyExtractor {
    type Key = IpAddr;

    #[allow(clippy::result_large_err)]
    fn extract<T>(
        &self,
        req: &http::Request<T>,
    ) -> Result<Self::Key, GovernorError> {
        match self.mode {
            RateLimitIpMode::Peer => self.extract_peer_ip(req),
            RateLimitIpMode::Smart => SmartIpKeyExtractor.extract(req),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        extract::ConnectInfo,
        http::{Request, header::HeaderName},
    };

    use super::*;

    fn req_with_connect_info(ip: [u8; 4]) -> Request<()> {
        let mut req = Request::builder()
            .uri("http://example.test/")
            .body(())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((ip, 12345))));
        req
    }

    #[test]
    fn peer_mode_ignores_forwarded_headers() {
        let mut req = req_with_connect_info([203, 0, 113, 10]);
        req.headers_mut().insert(
            HeaderName::from_static("x-forwarded-for"),
            "1.1.1.1".parse().unwrap(),
        );

        let extractor = RateLimitIpKeyExtractor::new(RateLimitIpMode::Peer);
        let ip = extractor.extract(&req).unwrap();
        assert_eq!(ip, IpAddr::from([203, 0, 113, 10]));
    }

    #[test]
    fn smart_mode_uses_x_forwarded_for_first() {
        let mut req = req_with_connect_info([203, 0, 113, 10]);
        req.headers_mut().insert(
            HeaderName::from_static("x-forwarded-for"),
            "1.1.1.1, 2.2.2.2".parse().unwrap(),
        );

        let extractor = RateLimitIpKeyExtractor::new(RateLimitIpMode::Smart);
        let ip = extractor.extract(&req).unwrap();
        assert_eq!(ip, IpAddr::from([1, 1, 1, 1]));
    }

    #[test]
    fn smart_mode_falls_back_to_peer_ip_when_headers_missing() {
        let req = req_with_connect_info([203, 0, 113, 10]);

        let extractor = RateLimitIpKeyExtractor::new(RateLimitIpMode::Smart);
        let ip = extractor.extract(&req).unwrap();
        assert_eq!(ip, IpAddr::from([203, 0, 113, 10]));
    }

    #[test]
    fn peer_mode_falls_back_to_socket_addr_extension() {
        let mut req = Request::builder()
            .uri("http://example.test/")
            .body(())
            .unwrap();
        req.extensions_mut()
            .insert(SocketAddr::from(([192, 0, 2, 1], 54321)));

        let extractor = RateLimitIpKeyExtractor::new(RateLimitIpMode::Peer);
        let ip = extractor.extract(&req).unwrap();
        assert_eq!(ip, IpAddr::from([192, 0, 2, 1]));
    }

    #[test]
    fn peer_mode_errors_without_peer_info() {
        let req = Request::builder()
            .uri("http://example.test/")
            .body(())
            .unwrap();

        let extractor = RateLimitIpKeyExtractor::new(RateLimitIpMode::Peer);
        let err = extractor.extract(&req).unwrap_err();
        assert!(matches!(err, GovernorError::UnableToExtractKey));
    }
}
