//! Control request for captive portals and a reference clock
//! (COMPAT-TRUST-CAUSES).
//!
//! A certificate error has two causes the certificate itself cannot show: the
//! network wants a login first (the portal answers every address, with its own
//! certificate), or the system clock is wrong. One plain-HTTP request to a
//! known address answers both: a portal replaces the expected reply, and the
//! `Date` header of any reply is a clock independent of this computer.
//!
//! Sent only when a certificate error is about to be shown, never on ordinary
//! navigation, over a direct connection (a portal lives on the local network;
//! a proxy or Tor would not see it) and with short timeouts.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::tls::failure::unix_from_civil;

/// Host and path of the control request (Mozilla's portal-detection endpoint:
/// answers `200` with the body `success`).
const PROBE_HOST: &str = "detectportal.firefox.com";
const PROBE_PATH: &str = "/success.txt";
const TIMEOUT: Duration = Duration::from_secs(2);

/// Result of one control request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalProbe {
    /// The reply was not the expected one: something on the way answers for
    /// the server.
    pub captive: bool,
    /// The reply's `Date` header, Unix seconds.
    pub server_date_unix: Option<i64>,
}

/// Run the control request. `None` — no reply at all (offline, DNS or connect
/// failure, timeout): nothing can be concluded.
#[must_use]
pub fn probe() -> Option<PortalProbe> {
    let addr = (PROBE_HOST, 80).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    let request = format!("GET {PROBE_PATH} HTTP/1.1\r\nHost: {PROBE_HOST}\r\nConnection: close\r\nAccept: */*\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = Vec::new();
    // A timeout mid-reply still leaves the headers read so far.
    let _ = stream.take(8 * 1024).read_to_end(&mut reply);
    parse_reply(&reply)
}

/// Judge a raw HTTP reply to the control request.
fn parse_reply(reply: &[u8]) -> Option<PortalProbe> {
    let text = String::from_utf8_lossy(reply);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    let date = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("date"))
        .and_then(|(_, v)| parse_http_date(v.trim()));
    Some(PortalProbe { captive: !(status == 200 && body.trim() == "success"), server_date_unix: date })
}

/// IMF-fixdate (`Tue, 14 Oct 2026 12:00:00 GMT`, RFC 9110 §5.6.7) → Unix seconds.
fn parse_http_date(s: &str) -> Option<i64> {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut parts = s.split_whitespace().skip(1); // weekday
    let day = parts.next()?.parse::<i64>().ok()?;
    let month_name = parts.next()?;
    let month = i64::try_from(MONTHS.iter().position(|m| *m == month_name)?).ok()? + 1;
    let year = parts.next()?.parse::<i64>().ok()?;
    let mut hms = parts.next()?.split(':').map(|n| n.parse::<i64>().ok());
    let (h, m, sec) = (hms.next()??, hms.next()??, hms.next()??);
    if !(1..=31).contains(&day) || h > 23 || m > 59 || sec > 60 || parts.next()? != "GMT" {
        return None;
    }
    Some(unix_from_civil(year, month, day) + h * 3_600 + m * 60 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_date_parses() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Wed, 01 Jan 2025 12:30:15 GMT"), Some(1_735_734_615));
        assert_eq!(parse_http_date("garbage"), None);
        assert_eq!(parse_http_date("Wed, 01 Foo 2025 12:30:15 GMT"), None);
    }

    #[test]
    fn expected_reply_is_not_a_portal() {
        let reply = b"HTTP/1.1 200 OK\r\nDate: Thu, 01 Jan 1970 00:00:10 GMT\r\nContent-Length: 8\r\n\r\nsuccess\n";
        assert_eq!(parse_reply(reply), Some(PortalProbe { captive: false, server_date_unix: Some(10) }));
    }

    #[test]
    fn redirect_or_foreign_body_is_a_portal() {
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: http://login.hotel.example/\r\n\r\n";
        assert_eq!(parse_reply(redirect), Some(PortalProbe { captive: true, server_date_unix: None }));
        let login = b"HTTP/1.1 200 OK\r\n\r\n<html>Please sign in</html>";
        assert_eq!(parse_reply(login).map(|p| p.captive), Some(true));
    }

    #[test]
    fn empty_reply_concludes_nothing() {
        assert_eq!(parse_reply(b""), None);
    }
}
