//! Shared outbound transport. DNS answers are checked by the resolver that
//! supplies the actual connection addresses, including redirected requests.
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{io, net::IpAddr, sync::Arc, time::Duration};
use url::{Host, Url};

#[derive(Debug, Clone, Copy, Default)]
pub struct OutboundPolicy {
    pub allow_private: bool,
}

impl OutboundPolicy {
    pub fn validate_host(&self, host: &str) -> Result<(), String> {
        if !self.allow_private
            && host
                .parse::<IpAddr>()
                .is_ok_and(|ip| is_private_or_reserved_ip(&ip))
        {
            return Err("Endpoint resolves to a private or reserved address".into());
        }
        Ok(())
    }

    /// Resolve once and connect to those exact checked addresses (also for MQTT).
    pub async fn connect_tcp(
        &self,
        host: &str,
        port: u16,
    ) -> Result<tokio::net::TcpStream, String> {
        self.validate_host(host)?;
        let addresses: Vec<_> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "Outbound DNS lookup failed")?
            .collect();
        if addresses.is_empty()
            || (!self.allow_private && addresses.iter().any(|a| is_private_or_reserved_ip(&a.ip())))
        {
            return Err("Blocked outbound DNS address".into());
        }
        tokio::net::TcpStream::connect(addresses.as_slice())
            .await
            .map_err(|_| "Outbound TCP connection failed".into())
    }

    pub fn validate(&self, url: &Url) -> Result<(), String> {
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            return Err("An absolute HTTP or HTTPS endpoint is required".into());
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err("Credentials and fragments are not allowed in endpoint URLs".into());
        }
        let ip = match url.host() {
            Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
            _ => None,
        };
        if !self.allow_private && ip.is_some_and(|ip| is_private_or_reserved_ip(&ip)) {
            return Err("Endpoint resolves to a private or reserved address".into());
        }
        Ok(())
    }
}

pub fn is_private_or_reserved_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
                || ip.is_documentation()
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback() || ip.is_unspecified() || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some_and(|v4| is_private_or_reserved_ip(&IpAddr::V4(v4)))
                // Only global unicast; reject special transition/translation ranges.
                || (first & 0xe000) != 0x2000
                || (first == 0x2001 && ip.segments()[1] == 0x0db8)
                || first == 0x2002
        }
    }
}

#[derive(Debug)]
struct CheckedResolver(OutboundPolicy);

impl Resolve for CheckedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.0;
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if addresses.is_empty()
                || (!policy.allow_private
                    && addresses.iter().any(|a| is_private_or_reserved_ip(&a.ip())))
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Blocked outbound DNS address",
                )
                .into());
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

#[derive(Clone, Debug)]
pub struct OutboundClient {
    client: reqwest::Client,
    policy: OutboundPolicy,
    timeout: Duration,
}

impl OutboundClient {
    pub fn new(policy: OutboundPolicy) -> Result<Self, reqwest::Error> {
        Self::with_timeout(policy, Duration::from_secs(10))
    }
    /// Context retrieval sends no receiver credentials or request body.
    pub fn for_contexts(policy: OutboundPolicy) -> Result<Self, reqwest::Error> {
        Self::build(policy, true, Duration::from_secs(10))
    }
    pub fn with_timeout(policy: OutboundPolicy, timeout: Duration) -> Result<Self, reqwest::Error> {
        Self::build(policy, false, timeout)
    }
    fn build(
        policy: OutboundPolicy,
        follow_redirects: bool,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(timeout)
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(8)
            .dns_resolver(Arc::new(CheckedResolver(policy)))
            // Never forward receiver credentials or JSON bodies through redirects.
            .redirect(if follow_redirects {
                reqwest::redirect::Policy::custom(move |attempt| {
                    if attempt.previous().len() >= 5 {
                        return attempt.error("Too many context redirects");
                    }
                    match policy.validate(attempt.url()) {
                        Ok(()) => attempt.follow(),
                        Err(e) => attempt.error(e),
                    }
                })
            } else {
                reqwest::redirect::Policy::none()
            })
            .build()?;
        Ok(Self {
            client,
            policy,
            timeout,
        })
    }

    pub fn policy(&self) -> OutboundPolicy {
        self.policy
    }
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn request(
        &self,
        method: reqwest::Method,
        endpoint: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let url = Url::parse(endpoint).map_err(|_| "Invalid endpoint URL".to_string())?;
        self.policy.validate(&url)?;
        Ok(self.client.request(method, url))
    }

    pub fn validate(&self, endpoint: &str) -> Result<(), String> {
        let url = Url::parse(endpoint).map_err(|_| "Invalid endpoint URL".to_string())?;
        self.policy.validate(&url)
    }
}

pub async fn bounded_json(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<serde_json::Value, String> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Remote JSON body exceeds the configured limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("Remote JSON body exceeds the configured limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_policy_checks_literals_and_schemes_even_in_development() {
        let policy = OutboundPolicy::default();
        for endpoint in [
            "http://127.0.0.1",
            "http://[::1]",
            "http://[fd00::1]",
            "http://[fe80::1]",
            "http://169.254.169.254",
            "http://[::ffff:127.0.0.1]",
            "file:///etc/passwd",
            "https://user:password@example.com",
        ] {
            assert!(
                policy.validate(&Url::parse(endpoint).unwrap()).is_err(),
                "{endpoint}"
            );
        }
        assert!(policy
            .validate(&Url::parse("https://example.com").unwrap())
            .is_ok());
        assert!(OutboundPolicy {
            allow_private: true
        }
        .validate(&Url::parse("file:///tmp/test").unwrap())
        .is_err());
    }
}
