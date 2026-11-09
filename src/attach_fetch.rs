//! The url of joppa_attach: one HTTPS GET of a public address, at most 3 redirects, 50 MiB
//! and 30 seconds. The server resolves each host one time, refuses a private address and
//! connects only to the checked addresses, so a second DNS answer cannot move the request.
use crate::spaces::ATTACHMENT_MAX;
use anyhow::{Context, Result, anyhow, ensure};
use reqwest::{Url, header, redirect::Policy};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

/// The test hook: the one origin that the fetch can reach over HTTP and on loopback.
/// Production never sets it.
const TEST_ORIGIN: &str = "JOPPA_ATTACH_URL_TEST_ORIGIN";
const PRIVATE: &str = "url resolves to a private address";
const REDIRECTS: usize = 3;

/// The bytes of `url` and the Content-Type of the reply as sent. The 30 seconds cover the
/// whole fetch, from the first connect to the last byte.
pub async fn fetch(url: &str) -> Result<(Vec<u8>, Option<String>)> {
    let url = Url::parse(url).context("url is not a valid URL")?;
    tokio::time::timeout(Duration::from_secs(30), follow(url))
        .await
        .map_err(|_| anyhow!("url did not finish in 30 seconds"))?
}

async fn follow(mut url: Url) -> Result<(Vec<u8>, Option<String>)> {
    for hop in 0.. {
        let mut response = get(&url).await?;
        if response.status().is_redirection()
            && let Some(location) = response.headers().get(header::LOCATION)
        {
            ensure!(hop < REDIRECTS, "url has more than 3 redirects");
            let location = location.to_str().context("url gave a bad redirect")?;
            url = url.join(location).context("url gave a bad redirect")?;
            continue;
        }
        let status = response.status();
        ensure!(status.is_success(), "url gave HTTP {}", status.as_u16());
        let too_big = || anyhow!("an attachment is at most 50 MiB");
        if response
            .content_length()
            .is_some_and(|n| n > ATTACHMENT_MAX as u64)
        {
            return Err(too_big());
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(read_error)? {
            if bytes.len() + chunk.len() > ATTACHMENT_MAX {
                return Err(too_big());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok((bytes, content_type));
    }
    unreachable!("the loop returns")
}

/// One GET without redirects, through a client that knows only the checked addresses.
async fn get(url: &Url) -> Result<reqwest::Response> {
    let test = std::env::var(TEST_ORIGIN).ok();
    let test = test.and_then(|origin| Url::parse(&origin).ok());
    let trusted = test.is_some_and(|test| url.origin() == test.origin());
    ensure!(
        url.scheme() == "https" || (trusted && url.scheme() == "http"),
        "url must use https"
    );
    let port = url.port_or_known_default().context("url has no port")?;
    let mut client = reqwest::Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(30));
    let host = url.host_str().context("url has no host")?;
    match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) => ensure!(trusted || public(ip), PRIVATE),
        Err(_) => {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| anyhow!("url host {host} does not resolve"))?
                .collect();
            ensure!(!addrs.is_empty(), "url host {host} does not resolve");
            // A name with any private address is refused, even when another is public.
            ensure!(trusted || addrs.iter().all(|a| public(a.ip())), PRIVATE);
            client = client.resolve_to_addrs(host, &addrs);
        }
    }
    client
        .build()?
        .get(url.clone())
        .send()
        .await
        .map_err(read_error)
}

fn read_error(e: reqwest::Error) -> anyhow::Error {
    anyhow!("url could not be read: {}", e.without_url())
}

/// False for an address that is not on the public internet.
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => public_v6(ip),
        },
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_loopback()
        || a == 0
        || ip.is_private()
        || ip.is_link_local()
        || (a == 100 && (64..128).contains(&b))
        || ip.is_multicast()
        || a >= 240)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // The NAT64 prefix 64:ff9b::/96 carries an IPv4 address in its last 32 bits.
    if ip.segments()[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [.., a, b, c, d] = ip.octets();
        return public_v4(Ipv4Addr::new(a, b, c, d));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || first & 0xfe00 == 0xfc00
        || first & 0xffc0 == 0xfe80
        || ip.is_multicast())
}
