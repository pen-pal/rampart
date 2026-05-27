//! Outbound proxies used by HTTP-family probes.
//!
//! Stored credentials are returned with the password field redacted —
//! the API tier serialises `Proxy` everywhere, so we keep the leak
//! surface tiny by stripping it at the deserialise/serialise boundary.

use crate::ids::ProxyId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks,
    Socks5,
    Socks4,
}

impl ProxyProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProxyProtocol::Http   => "http",
            ProxyProtocol::Https  => "https",
            ProxyProtocol::Socks  => "socks",
            ProxyProtocol::Socks5 => "socks5",
            ProxyProtocol::Socks4 => "socks4",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "http"   => Some(ProxyProtocol::Http),
            "https"  => Some(ProxyProtocol::Https),
            "socks"  => Some(ProxyProtocol::Socks),
            "socks5" => Some(ProxyProtocol::Socks5),
            "socks4" => Some(ProxyProtocol::Socks4),
            _        => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    pub id:         ProxyId,
    pub protocol:   String,
    pub host:       String,
    pub port:       i32,
    pub auth:       bool,
    pub username:   Option<String>,
    /// Always None on the API surface — passwords never leave the DB.
    /// The probe runtime fetches them separately via a dedicated lookup.
    #[serde(skip_serializing)]
    pub password:   Option<String>,
    pub active:     bool,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewProxy {
    #[validate(length(min = 1, max = 16))]
    pub protocol: String,

    #[validate(length(min = 1, max = 255))]
    pub host: String,

    #[validate(range(min = 1, max = 65535))]
    pub port: i32,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub password: Option<String>,

    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool { true }
