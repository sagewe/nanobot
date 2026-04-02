use std::net::{IpAddr, SocketAddr};

use anyhow::{Result, anyhow, bail};
use regex::Regex;
use url::Url;

pub fn contains_internal_url(input: &str) -> bool {
    let regex = Regex::new(r#"https?://[^\s"')>]+"#).expect("valid url regex");
    regex.find_iter(input).any(|mat| {
        validate_url_target(mat.as_str())
            .map(|allowed| !allowed)
            .unwrap_or(false)
    })
}

pub fn validate_url_target(raw: &str) -> Result<bool> {
    let url = Url::parse(raw)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Ok(false);
    }
    let Some(host) = url.host_str() else {
        return Ok(true);
    };
    if is_blocked_hostname(host) {
        return Ok(false);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(!is_private_ip(ip));
    }
    Ok(true)
}

pub async fn validate_web_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw)?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only http/https URLs are supported");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL is missing a host"))?;
    if is_blocked_hostname(host) {
        bail!("URL target is internal/private");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            bail!("URL target is internal/private");
        }
        return Ok(url);
    }

    let port = url.port_or_known_default().unwrap_or(80);
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| anyhow!("failed to resolve host '{host}': {error}"))?
        .collect::<Vec<SocketAddr>>();
    if resolved.is_empty() {
        bail!("host '{host}' did not resolve to any address");
    }
    if !resolved_addresses_are_public(&resolved) {
        bail!("URL target resolves to an internal/private address");
    }
    Ok(url)
}

fn is_blocked_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost")
}

fn resolved_addresses_are_public(addresses: &[SocketAddr]) -> bool {
    addresses.iter().all(|address| !is_private_ip(address.ip()))
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_literal_private_targets() {
        assert!(!validate_url_target("http://127.0.0.1:8080").expect("validate"));
        assert!(!validate_url_target("http://localhost:8080").expect("validate"));
        assert!(validate_url_target("https://example.com").expect("validate"));
    }

    #[test]
    fn rejects_private_resolved_addresses() {
        let addresses = vec![
            "127.0.0.1:80".parse::<SocketAddr>().expect("addr"),
            "[::1]:80".parse::<SocketAddr>().expect("addr"),
        ];
        assert!(!resolved_addresses_are_public(&addresses));

        let public = vec!["93.184.216.34:443".parse::<SocketAddr>().expect("addr")];
        assert!(resolved_addresses_are_public(&public));
    }
}
