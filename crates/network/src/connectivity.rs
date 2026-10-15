//! "Is this computer online at all?" (COMPAT-ERRORS, COMPAT-ERR-01).
//!
//! A navigation that fails with a DNS, timeout or unreachable-network error
//! looks the same whether the site is down or the computer has no internet.
//! The two need different advice, so before the error page is shown one TCP
//! connect goes to each of a few unrelated well-known public addresses (plain
//! IPs: no DNS involved, so a dead resolver cannot hide a working link). Only
//! when *every* attempt fails is the machine called offline; a single answer
//! (even from behind a captive portal) means the link is up.
//!
//! Runs only on a failed main-document navigation, never on a normal one,
//! over a direct connection (a proxy or Tor would make the verdict wrong and
//! Tor must not open extra direct connections), in parallel with a short
//! timeout: the worst case adds [`TIMEOUT`] to an error that already took
//! longer than that.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// Per-address connect timeout; all addresses are tried at once.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Unrelated operators on unrelated networks, so one blocked or down address
/// (a country blocking Google, a firewall dropping Cloudflare) cannot make an
/// online computer look offline: Cloudflare, Google, Quad9 (HTTPS ports) and
/// Yandex DNS (port 53).
const TARGETS: [(Ipv4Addr, u16); 4] = [
    (Ipv4Addr::new(1, 1, 1, 1), 443),
    (Ipv4Addr::new(8, 8, 8, 8), 443),
    (Ipv4Addr::new(9, 9, 9, 9), 443),
    (Ipv4Addr::new(77, 88, 8, 8), 53),
];

/// `true` when none of the well-known public addresses accepts a connection.
#[must_use]
pub fn is_offline() -> bool {
    let targets: Vec<SocketAddr> = TARGETS.iter().map(|(ip, port)| SocketAddr::new(IpAddr::V4(*ip), *port)).collect();
    none_reachable(&targets, |addr| TcpStream::connect_timeout(&addr, TIMEOUT).is_ok())
}

/// `true` when `connect` fails for every target. Each target gets its own
/// thread; the first success ends the wait without waiting for the rest.
fn none_reachable(targets: &[SocketAddr], connect: impl Fn(SocketAddr) -> bool + Send + Sync + Clone + 'static) -> bool {
    if targets.is_empty() {
        return false;
    }
    let (tx, rx) = mpsc::channel();
    for &addr in targets {
        let tx = tx.clone();
        let connect = connect.clone();
        std::thread::spawn(move || {
            let _ = tx.send(connect(addr));
        });
    }
    drop(tx);
    // `iter` ends once every sender is gone, i.e. all attempts have failed.
    !rx.iter().any(|ok| ok)
}

/// Hosts whose failure says nothing about the internet: the machine itself, a
/// private or link-local address, a single-label name or an intranet suffix.
#[must_use]
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
            IpAddr::V6(v6) => {
                let first = v6.segments()[0];
                v6.is_loopback() || v6.is_unspecified() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
            }
        };
    }
    !host.contains('.') || [".local", ".lan", ".internal", ".intranet", ".home", ".corp"].iter().any(|s| host.ends_with(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs(n: u8) -> Vec<SocketAddr> {
        (0..n).map(|i| SocketAddr::from(([10, 0, 0, i], 443))).collect()
    }

    #[test]
    fn offline_only_when_every_attempt_fails() {
        assert!(none_reachable(&addrs(4), |_| false));
    }

    #[test]
    fn one_answer_is_enough_to_be_online() {
        assert!(!none_reachable(&addrs(4), |a| a.ip() == IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))));
        assert!(!none_reachable(&addrs(4), |_| true));
    }

    #[test]
    fn no_targets_is_no_verdict() {
        assert!(!none_reachable(&[], |_| false));
    }

    #[test]
    fn local_hosts_are_recognised() {
        for h in [
            "localhost", "app.localhost", "127.0.0.1", "192.168.1.5", "10.1.2.3", "169.254.0.9", "[::1]", "fe80::1",
            "fd00::5", "nas", "printer.local", "wiki.corp",
        ] {
            assert!(is_local_host(h), "{h}");
        }
        for h in ["example.com", "93.184.216.34", "2606:4700::1111", "sub.example.co.uk"] {
            assert!(!is_local_host(h), "{h}");
        }
        assert!(is_local_host(""));
    }
}
