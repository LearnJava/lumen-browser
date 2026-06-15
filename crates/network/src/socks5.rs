//! SOCKS5 proxy client — RFC 1928.
//!
//! Handles the initial SOCKS5 handshake over an already-established TCP
//! connection to the proxy server.  After a successful call to
//! [`socks5_connect`] the stream is tunnelled to the requested target and
//! can be used for plain HTTP or wrapped in TLS for HTTPS.
//!
//! Supported authentication methods:
//! - No authentication (RFC 1928 §3, method 0x00)
//! - Username / password (RFC 1929, method 0x02)
//!
//! Only `CONNECT` command is used (CMD=0x01); BIND and UDP ASSOCIATE are
//! not implemented and not needed for browser traffic.

use std::io::{self, Read, Write};
use std::net::TcpStream;

use crate::Error;

/// SOCKS5 proxy server address and optional credentials.
#[derive(Debug, Clone)]
pub struct Socks5Proxy {
    /// Proxy hostname or IP address (e.g. `"127.0.0.1"` for local Tor).
    pub host: String,
    /// Proxy port (Tor daemon default: 9050; Tor Browser bundle: 9150).
    pub port: u16,
    /// Optional (username, password) pair for RFC 1929 auth.
    pub auth: Option<(String, String)>,
}

impl Socks5Proxy {
    /// Create a new SOCKS5 proxy without authentication.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            auth: None,
        }
    }

    /// Attach username / password credentials (RFC 1929).
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = Some((username.into(), password.into()));
        self
    }
}

/// Perform a SOCKS5 handshake on `stream` and request a `CONNECT` to
/// `target_host:target_port`.
///
/// On success the stream is tunnelled to the target and returned unchanged.
/// The caller can then initiate TLS over it (for HTTPS) or send HTTP/1.1
/// directly (for plain HTTP).
///
/// `stream` must already be TCP-connected to the SOCKS5 proxy server.
pub fn socks5_connect(
    mut stream: TcpStream,
    target_host: &str,
    target_port: u16,
    auth: Option<&(String, String)>,
) -> Result<TcpStream, Error> {
    // ── Phase 1: Method negotiation ──────────────────────────────────────
    // Advertise: no-auth (0x00) always; also username/password (0x02) when
    // credentials are provided.
    let methods: Vec<u8> = if auth.is_some() {
        vec![0x00, 0x02]
    } else {
        vec![0x00]
    };

    let mut greeting = Vec::with_capacity(2 + methods.len());
    greeting.push(0x05); // SOCKS version
    greeting.push(methods.len() as u8);
    greeting.extend_from_slice(&methods);
    stream
        .write_all(&greeting)
        .map_err(|e| socks_io_err("write greeting", e))?;

    // Server selects a method.
    let mut method_resp = [0u8; 2];
    stream
        .read_exact(&mut method_resp)
        .map_err(|e| socks_io_err("read method", e))?;
    if method_resp[0] != 0x05 {
        return Err(Error::Network(format!(
            "SOCKS5: unexpected server version {}",
            method_resp[0]
        )));
    }

    match method_resp[1] {
        0x00 => {
            // No authentication required — proceed.
        }
        0x02 => {
            // Username / password authentication (RFC 1929).
            let (user, pass) = auth.ok_or_else(|| {
                Error::Network("SOCKS5: server requires auth but no credentials given".to_string())
            })?;
            do_username_password_auth(&mut stream, user, pass)?;
        }
        0xFF => {
            return Err(Error::Network(
                "SOCKS5: no acceptable authentication method".to_string(),
            ));
        }
        m => {
            return Err(Error::Network(format!(
                "SOCKS5: unsupported auth method 0x{m:02x}"
            )));
        }
    }

    // ── Phase 2: CONNECT request ─────────────────────────────────────────
    // Use DOMAINNAME (0x03) address type so the proxy resolves DNS — this
    // is essential for Tor (DNS must not leak to the local network).
    let host_bytes = target_host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(Error::Network(format!(
            "SOCKS5: hostname too long ({} bytes)",
            host_bytes.len()
        )));
    }

    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.push(0x05); // version
    req.push(0x01); // CMD = CONNECT
    req.push(0x00); // RSV
    req.push(0x03); // ATYP = DOMAINNAME
    req.push(host_bytes.len() as u8);
    req.extend_from_slice(host_bytes);
    req.push((target_port >> 8) as u8);
    req.push((target_port & 0xFF) as u8);
    stream
        .write_all(&req)
        .map_err(|e| socks_io_err("write connect request", e))?;

    // ── Phase 3: CONNECT response ────────────────────────────────────────
    // Response: [VER, REP, RSV, ATYP, BND.ADDR (variable), BND.PORT (2)]
    let mut resp_hdr = [0u8; 4];
    stream
        .read_exact(&mut resp_hdr)
        .map_err(|e| socks_io_err("read connect response", e))?;

    if resp_hdr[0] != 0x05 {
        return Err(Error::Network(format!(
            "SOCKS5: unexpected version in response: {}",
            resp_hdr[0]
        )));
    }
    if resp_hdr[1] != 0x00 {
        return Err(Error::Network(format!(
            "SOCKS5: CONNECT failed, REP=0x{:02x} ({})",
            resp_hdr[1],
            socks5_rep_message(resp_hdr[1])
        )));
    }

    // Drain bound address from the response (not used, but must be consumed).
    drain_bound_addr(&mut stream, resp_hdr[3])?;

    Ok(stream)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// RFC 1929: sub-negotiation for username / password authentication.
fn do_username_password_auth(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<(), Error> {
    let u = username.as_bytes();
    let p = password.as_bytes();
    if u.len() > 255 || p.len() > 255 {
        return Err(Error::Network(
            "SOCKS5: credentials too long for RFC 1929".to_string(),
        ));
    }
    let mut msg = Vec::with_capacity(3 + u.len() + p.len());
    msg.push(0x01); // sub-negotiation version
    msg.push(u.len() as u8);
    msg.extend_from_slice(u);
    msg.push(p.len() as u8);
    msg.extend_from_slice(p);
    stream
        .write_all(&msg)
        .map_err(|e| socks_io_err("write auth", e))?;

    let mut resp = [0u8; 2];
    stream
        .read_exact(&mut resp)
        .map_err(|e| socks_io_err("read auth response", e))?;
    if resp[1] != 0x00 {
        return Err(Error::Network(format!(
            "SOCKS5: authentication failed (status {})",
            resp[1]
        )));
    }
    Ok(())
}

/// Read and discard the bound-address field from a CONNECT response.
fn drain_bound_addr(stream: &mut TcpStream, atyp: u8) -> Result<(), Error> {
    let addr_len = match atyp {
        0x01 => 4,  // IPv4
        0x04 => 16, // IPv6
        0x03 => {
            let mut len_buf = [0u8; 1];
            stream
                .read_exact(&mut len_buf)
                .map_err(|e| socks_io_err("read bound addr len", e))?;
            len_buf[0] as usize
        }
        other => {
            return Err(Error::Network(format!(
                "SOCKS5: unknown ATYP in response: 0x{other:02x}"
            )));
        }
    };
    let mut buf = vec![0u8; addr_len + 2]; // addr + 2-byte port
    stream
        .read_exact(&mut buf)
        .map_err(|e| socks_io_err("read bound addr", e))?;
    Ok(())
}

fn socks_io_err(ctx: &str, e: io::Error) -> Error {
    Error::Network(format!("SOCKS5 {ctx}: {e}"))
}

/// Human-readable SOCKS5 REP field description.
fn socks5_rep_message(rep: u8) -> &'static str {
    match rep {
        0x01 => "general SOCKS server failure",
        0x02 => "connection not allowed by ruleset",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown error",
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socks5_proxy_new_no_auth() {
        let p = Socks5Proxy::new("127.0.0.1", 9050);
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 9050);
        assert!(p.auth.is_none());
    }

    #[test]
    fn socks5_proxy_with_auth() {
        let p = Socks5Proxy::new("proxy.example.com", 1080).with_auth("user", "pass");
        assert_eq!(p.host, "proxy.example.com");
        assert_eq!(p.port, 1080);
        let (u, pw) = p.auth.unwrap();
        assert_eq!(u, "user");
        assert_eq!(pw, "pass");
    }

    #[test]
    fn socks5_rep_messages_known() {
        assert_eq!(socks5_rep_message(0x01), "general SOCKS server failure");
        assert_eq!(socks5_rep_message(0x05), "connection refused");
        assert_eq!(socks5_rep_message(0xFF), "unknown error");
    }
}
