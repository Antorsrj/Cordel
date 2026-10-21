//! The client address, for rate limiting.
//!
//! The service binds to loopback and the reverse proxy connects from
//! loopback, so a forwarded address is only trusted when the connection itself
//! came from loopback. Any other connection uses the socket address, which no
//! client can forge.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

/// The address a request came from, when it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let socket = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        // A forwarded address is only believable when the peer is the local
        // proxy. Otherwise a client could name any address it liked.
        let forwarded = match socket {
            Some(ip) if ip.is_loopback() => parts
                .headers
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(',').next())
                .and_then(|value| value.trim().parse::<IpAddr>().ok()),
            _ => None,
        };
        Ok(ClientIp(forwarded.or(socket)))
    }
}
