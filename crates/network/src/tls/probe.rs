//! TLS trust probe: one handshake against a host, report what the server's
//! certificate chain says about who vouches for it (COMPAT-CORPUS-5, the
//! "trust robot" of `docs/research/site-compat/report.md` §6.2/§7.6 п.2).
//!
//! The browser keeps none of this once a connection is up: `CertInfo` carries
//! the leaf, not the chain, and the stapled OCSP bytes only ever reach the
//! verifier ([`super::verifier`] module docs). The probe therefore wraps
//! [`LumenVerifier`] in a recorder that
//!
//! - runs the real verdict (same roots as browsing: Mozilla + OS store),
//! - keeps the chain as sent and the OCSP verdict,
//! - then **accepts the chain regardless**, so a host whose CA is not trusted
//!   yet (НУЦ, an expired certificate, an inspecting antivirus) still yields
//!   its issuer instead of a bare "UnknownIssuer". The trust verdict is
//!   reported separately in [`TlsProbe::trusted`].
//!
//! Accepting is safe because a probe never sends application data: the
//! connection is dropped right after the handshake. The recorder is built per
//! probe, so the "last seen" slot the verifier docs warn about is never shared
//! between connections.
//!
//! What is *not* measured: the number of SCTs from distinct **operators**.
//! [`super::ct_logs::KNOWN_LOG_IDS`] is empty (no log → operator table yet), so
//! the probe reports the distinct SCT **log ids** instead; once the list is
//! vendored, the operator count is a lookup over [`TlsProbe::sct_log_ids`].

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConnection, DigitallySignedStruct, Error as RustlsError, SignatureScheme};

use super::cert_fields::{self, LeafFields};
use super::ocsp::{self, OcspVerdict};
use super::{ct, os_roots, LumenVerifier, TlsProfile};

/// Connect and handshake budget per host.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Which store anchors the chain the server sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// A root of the Mozilla list shipped in the build (`webpki-roots`).
    Mozilla,
    /// A root that exists only in the operating system store.
    Os,
    /// Russian state CA (НУЦ Минцифры): `Russian Trusted Root/Sub CA`.
    Nuc,
    /// None of the above (private CA, antivirus root, incomplete chain).
    Unknown,
}

impl RootKind {
    /// Stable label for the JSON report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mozilla => "webpki",
            Self::Os => "os",
            Self::Nuc => "nuc",
            Self::Unknown => "unknown",
        }
    }
}

/// One host's probe result. Every field is plain data so the report is stable
/// and diff-able day to day.
#[derive(Debug, Clone)]
pub struct TlsProbe {
    /// Host as given (without scheme and port).
    pub host: String,
    /// TCP port probed.
    pub port: u16,
    /// A certificate chain was received (even if it is not trusted).
    pub seen_chain: bool,
    /// The handshake ran to completion.
    pub handshake_ok: bool,
    /// DNS, connect or handshake failure; empty on success.
    pub error: String,
    /// `LumenVerifier` accepted the chain (roots as in browsing).
    pub trusted: bool,
    /// Why it did not, empty when trusted.
    pub trust_error: String,
    /// `TLS 1.3` / `TLS 1.2`; empty when the handshake did not finish.
    pub tls_version: String,
    /// Chain as sent, leaf first.
    pub chain: Vec<LeafFields>,
    /// SHA-256 of the leaf, colon-separated hex.
    pub leaf_sha256: String,
    /// Which store anchors the chain.
    pub root_kind: RootKind,
    /// `issuer` of the last certificate sent — the root's name for chains
    /// that omit the root.
    pub anchor_name: String,
    /// Distinct embedded-SCT log ids of the leaf, lowercase hex, sorted.
    pub sct_log_ids: Vec<String>,
    /// `good` (stapled, not revoked), `revoked`, or `no_staple`.
    pub revocation: &'static str,
    /// Wall time of the probe.
    pub elapsed_ms: u64,
}

impl TlsProbe {
    fn empty(host: &str, port: u16) -> Self {
        Self {
            host: host.to_owned(),
            port,
            seen_chain: false,
            handshake_ok: false,
            error: String::new(),
            trusted: false,
            trust_error: String::new(),
            tls_version: String::new(),
            chain: Vec::new(),
            leaf_sha256: String::new(),
            root_kind: RootKind::Unknown,
            anchor_name: String::new(),
            sct_log_ids: Vec::new(),
            revocation: "no_staple",
            elapsed_ms: 0,
        }
    }

    /// One line of JSON (no trailing newline): the unit the daily database
    /// stores and `scripts/tls_trust_robot.py` diffs.
    pub fn to_json_line(&self) -> String {
        let leaf = self.chain.first().cloned().unwrap_or_default();
        let mut out = String::with_capacity(512);
        out.push('{');
        push_str_field(&mut out, "host", &self.host, true);
        out.push_str(&format!(",\"port\":{}", self.port));
        out.push_str(&format!(",\"handshake_ok\":{}", self.handshake_ok));
        out.push_str(&format!(",\"seen_chain\":{}", self.seen_chain));
        push_str_field(&mut out, "error", &self.error, false);
        out.push_str(&format!(",\"trusted\":{}", self.trusted));
        push_str_field(&mut out, "trust_error", &self.trust_error, false);
        push_str_field(&mut out, "tls_version", &self.tls_version, false);
        push_str_field(&mut out, "issuer_cn", &leaf.issuer_cn, false);
        push_str_field(&mut out, "issuer_org", &leaf.issuer_org, false);
        push_str_field(&mut out, "subject_cn", &leaf.subject_cn, false);
        push_str_field(&mut out, "not_after", &leaf.not_after, false);
        push_str_field(&mut out, "leaf_sha256", &self.leaf_sha256, false);
        push_str_field(&mut out, "root_kind", self.root_kind.as_str(), false);
        push_str_field(&mut out, "anchor_name", &self.anchor_name, false);
        out.push_str(&format!(",\"chain_len\":{}", self.chain.len()));
        out.push_str(",\"chain_issuers\":[");
        for (i, c) in self.chain.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let name = if c.issuer_cn.is_empty() { &c.issuer_org } else { &c.issuer_cn };
            out.push('"');
            json_escape_into(name, &mut out);
            out.push('"');
        }
        out.push(']');
        out.push_str(&format!(",\"sct_logs\":{}", self.sct_log_ids.len()));
        out.push_str(",\"sct_log_ids\":[");
        for (i, id) in self.sct_log_ids.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(id);
            out.push('"');
        }
        out.push(']');
        push_str_field(&mut out, "revocation", self.revocation, false);
        out.push_str(&format!(",\"elapsed_ms\":{}", self.elapsed_ms));
        out.push('}');
        out
    }
}

fn push_str_field(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    out.push('"');
    out.push_str(key);
    out.push_str("\":\"");
    json_escape_into(value, out);
    out.push('"');
}

fn json_escape_into(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

/// Split `https://host:8443/path`, `host:443`, `host` into `(host, port)`.
/// Port defaults to 443. `None` for an empty host.
pub fn parse_target(target: &str) -> Option<(String, u16)> {
    let t = target.trim();
    let t = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://")).unwrap_or(t);
    let t = t.split(['/', '?', '#']).next().unwrap_or("");
    let t = t.rsplit_once('@').map_or(t, |(_, h)| h);
    if t.is_empty() {
        return None;
    }
    // `[::1]:443` — an IPv6 literal keeps its colons inside the brackets.
    if let Some(rest) = t.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = tail.strip_prefix(':').map_or(Some(443), |p| p.parse().ok())?;
        return Some((host.to_owned(), port));
    }
    match t.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !host.contains(':') => {
            Some((host.to_owned(), port.parse().ok()?))
        }
        _ if t.contains(':') => None,
        _ => Some((t.to_owned(), 443)),
    }
}

/// What the recorder saw during one handshake.
#[derive(Debug)]
struct Seen {
    chain: Vec<CertificateDer<'static>>,
    ocsp: OcspVerdict,
    trust_error: Option<String>,
}

/// `LumenVerifier` verdict recorded, chain accepted — see the module docs.
#[derive(Debug)]
struct RecordingVerifier {
    inner: Arc<LumenVerifier>,
    seen: Mutex<Option<Seen>>,
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let verdict =
            self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
        let mut chain = vec![end_entity.clone().into_owned()];
        chain.extend(intermediates.iter().map(|c| c.clone().into_owned()));
        let seen = Seen {
            chain,
            ocsp: ocsp::parse_stapled_response(ocsp_response),
            trust_error: verdict.err().map(|e| e.to_string()),
        };
        if let Ok(mut slot) = self.seen.lock() {
            *slot = Some(seen);
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Which store vouches for `chain` (leaf first, as sent): judged by the issuer and
/// subject of its last certificate. НУЦ first: its roots are not in the
/// Mozilla list and the OS-store loader refuses them (`os_roots` docs).
pub fn classify_anchor(chain: &[CertificateDer<'_>]) -> RootKind {
    let Some((issuer, subject)) = chain.last().and_then(|c| cert_fields::issuer_and_subject_der(c.as_ref())) else {
        return RootKind::Unknown;
    };
    if os_roots::is_nuc_subject(issuer) || os_roots::is_nuc_subject(subject) {
        return RootKind::Nuc;
    }
    let mozilla = webpki_roots::TLS_SERVER_ROOTS
        .iter()
        .any(|r| r.subject.as_ref() == issuer || r.subject.as_ref() == subject);
    if mozilla {
        RootKind::Mozilla
    } else if os_roots::local_root_name(chain).is_some() {
        RootKind::Os
    } else {
        RootKind::Unknown
    }
}

/// Build the report from what the recorder saw. Split out of [`probe_host`]
/// so it is testable on synthetic chains without a network.
fn fill_from_seen(p: &mut TlsProbe, seen: &Seen) {
    p.seen_chain = !seen.chain.is_empty();
    p.trusted = seen.trust_error.is_none();
    p.trust_error = seen.trust_error.clone().unwrap_or_default();
    p.chain = seen.chain.iter().map(|c| cert_fields::extract_leaf_fields(c.as_ref()).unwrap_or_default()).collect();
    if let Some(leaf) = seen.chain.first() {
        p.leaf_sha256 = cert_fields::sha256_fingerprint_hex(leaf.as_ref());
        let mut ids: Vec<String> = ct::extract_sct_log_ids(leaf.as_ref())
            .iter()
            .map(|id| id.iter().map(|b| format!("{b:02x}")).collect())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        p.sct_log_ids = ids;
    }
    p.root_kind = classify_anchor(&seen.chain);
    if let Some(last) = p.chain.last() {
        p.anchor_name = if last.issuer_cn.is_empty() { last.issuer_org.clone() } else { last.issuer_cn.clone() };
    }
    p.revocation = match seen.ocsp {
        OcspVerdict::Good => "good",
        OcspVerdict::Revoked => "revoked",
        OcspVerdict::Unknown => "no_staple",
    };
}

/// Handshake with `host:port` under the Standard (Chrome-like) profile and
/// report the chain. Never returns an error: a failure is data in
/// [`TlsProbe::error`].
pub fn probe_host(host: &str, port: u16) -> TlsProbe {
    let started = Instant::now();
    let mut probe = TlsProbe::empty(host, port);
    let recorder_slot: Arc<Mutex<Option<Arc<RecordingVerifier>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&recorder_slot);
    let config = super::build_client_config_wrapping(TlsProfile::Standard, super::trusted_root_store(), move |inner| {
        let rec = Arc::new(RecordingVerifier { inner, seen: Mutex::new(None) });
        if let Ok(mut s) = slot.lock() {
            *s = Some(Arc::clone(&rec));
        }
        rec
    });
    let recorder = recorder_slot.lock().ok().and_then(|s| s.clone());

    match handshake(host, port, Arc::new(config)) {
        Ok(version) => {
            probe.handshake_ok = true;
            probe.tls_version = version;
        }
        Err(e) => probe.error = e,
    }
    let seen = recorder.and_then(|r| r.seen.lock().ok().and_then(|mut s| s.take()));
    if let Some(seen) = seen {
        fill_from_seen(&mut probe, &seen);
    }
    probe.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    probe
}

fn handshake(host: &str, port: u16, config: Arc<rustls::ClientConfig>) -> Result<String, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS: {e}"))?
        .next()
        .ok_or_else(|| String::from("DNS: нет адресов"))?;
    let mut tcp = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).map_err(|e| format!("connect: {e}"))?;
    let _ = tcp.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(PROBE_TIMEOUT));
    let name = ServerName::try_from(host.to_owned()).map_err(|e| format!("host: {e}"))?;
    let mut conn = ClientConnection::new(config, name).map_err(|e| format!("TLS: {e}"))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| format!("TLS handshake: {e}"))?;
    }
    Ok(match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3",
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2",
        _ => "TLS",
    }
    .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_forms() {
        assert_eq!(parse_target("example.com"), Some(("example.com".into(), 443)));
        assert_eq!(parse_target("https://example.com/a?b#c"), Some(("example.com".into(), 443)));
        assert_eq!(parse_target("example.com:8443"), Some(("example.com".into(), 8443)));
        assert_eq!(parse_target("https://u:p@example.com:444/x"), Some(("example.com".into(), 444)));
        assert_eq!(parse_target("[::1]:9443"), Some(("::1".into(), 9443)));
        assert_eq!(parse_target("[::1]"), Some(("::1".into(), 443)));
    }

    #[test]
    fn parse_target_rejects_garbage() {
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("https:///x"), None);
        assert_eq!(parse_target("host:notaport"), None);
        assert_eq!(parse_target("::1"), None);
    }

    #[test]
    fn json_line_escapes_and_has_stable_keys() {
        let mut p = TlsProbe::empty("a\"b.example", 443);
        p.error = "line1\nline2".into();
        let j = p.to_json_line();
        assert!(j.starts_with("{\"host\":\"a\\\"b.example\",\"port\":443"));
        assert!(j.contains("\"error\":\"line1\\nline2\""));
        assert!(j.contains("\"revocation\":\"no_staple\""));
        assert!(j.ends_with('}'));
        assert!(!j.contains('\n'));
    }

    #[test]
    fn unreachable_port_reports_error_not_panic() {
        // Port 1 on loopback: nothing listens, connect is refused at once.
        let p = probe_host("127.0.0.1", 1);
        assert!(!p.handshake_ok);
        assert!(!p.seen_chain);
        assert!(p.error.starts_with("connect:"), "{}", p.error);
    }

    #[test]
    fn empty_chain_classifies_unknown() {
        assert_eq!(classify_anchor(&[]), RootKind::Unknown);
    }

    /// Loopback server with the WPT test certificate (`tests/wpt/certs`): the
    /// chain is not trusted, yet the probe must still report who issued it.
    #[test]
    fn untrusted_chain_is_recorded_not_lost() {
        use std::io::Write;
        let certs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/wpt/certs");
        let cert_pem = std::fs::read(certs_dir.join("host-cert.pem")).expect("host-cert.pem");
        let key_pem = std::fs::read(certs_dir.join("host-key.pem")).expect("host-key.pem");
        let chain: Vec<_> = rustls_pemfile::certs(&mut cert_pem.as_slice()).filter_map(Result::ok).collect();
        let key = rustls_pemfile::private_key(&mut key_pem.as_slice()).expect("key pem").expect("a key");
        let server_cfg = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .expect("cert+key"),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("accept");
            let mut conn = rustls::ServerConnection::new(server_cfg).expect("server conn");
            while conn.is_handshaking() {
                if conn.complete_io(&mut tcp).is_err() {
                    return;
                }
            }
            let _ = tcp.flush();
        });

        let p = probe_host("127.0.0.1", port);
        let _ = server.join();
        assert!(p.handshake_ok, "{}", p.error);
        assert!(p.seen_chain);
        assert!(!p.trusted, "a WPT test CA must not be trusted");
        assert!(!p.trust_error.is_empty());
        assert!(!p.chain.is_empty());
        assert!(!p.anchor_name.is_empty(), "issuer of the last certificate is reported");
        assert!(p.leaf_sha256.contains(':'));
        assert_eq!(p.root_kind, RootKind::Unknown);
        assert_eq!(p.revocation, "no_staple");
        assert!(p.to_json_line().contains("\"trusted\":false"));
    }
}
