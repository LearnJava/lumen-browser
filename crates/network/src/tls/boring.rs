//! BUG-1826: BoringSSL transport — Chrome-grade TLS ClientHello.
//!
//! rustls cannot emit what Cloudflare's edge expects from a Chromium client:
//! no GREASE, no extension permutation, a reduced cipher/extension set — the
//! JA3/JA4/peetprint fingerprints diverge enough that CF escalates to a
//! managed challenge the page then cannot finish. BoringSSL is the stack real
//! Chrome is built on: GREASE, permuted extensions, X25519MLKEM768 keyshare
//! and the full cipher/sigalg lists come for free.
//!
//! Certificate policy is unchanged: the handshake runs with
//! `SslVerifyMode::NONE` and the peer chain is verified **after** the
//! handshake by the same [`LumenVerifier`] the rustls path uses
//! ([`rustls::client::danger::ServerCertVerifier::verify_server_cert`] is a
//! pure function of DER bytes, SNI and the stapled OCSP response — all
//! obtainable from a completed `SslStream`). This preserves webpki path
//! validation, AIA issuer lookup, the national-CA (NUC) policy, the
//! stapled-OCSP `Revoked` hard-fail, CT requirements and the per-host
//! "Proceed anyway" bypass with zero porting. The cost: a chain that fails
//! policy completes the TLS handshake before being rejected — no application
//! data is exchanged in between, which is the same exposure the bypass flow
//! already has.
//!
//! h3/QUIC and DNS-over-TLS stay on rustls: QUIC has no BoringSSL backend
//! here, and DoT never reaches a website's edge fingerprint.

use std::collections::HashMap;
use std::io;
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock};

use boring::ssl::{
    CertificateCompressionAlgorithm, CertificateCompressor, HandshakeError, SslConnector, SslMethod,
    SslStream, SslVerifyMode, SslVersion, StatusType,
};
use boring_sys as ffi;
use foreign_types::ForeignType;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

use super::{LumenVerifier, TlsProfile, cert_error, ct, nuc, ocsp};
use crate::{Error, NetErrorCode, net_failure};

/// TLS ClientHello-списки Chrome берутся из `crate::http::chrome_manifest::CHROME_157.tls`
/// (FINGERPRINT-MANIFEST): cipher_list (TLS 1.2 + 1.3 AEAD в порядке
/// предпочтения Chrome; TLS 1.3 suites в BoringSSL неявные), sigalgs
/// (ML-DSA первыми, без `rsa_pkcs1_sha1`), groups (X25519MLKEM768 первым,
/// Chrome ≥124), ALPN. Значения перенесены без изменений — рефакторинг
/// WS-B не должен двигать wire.
const TLS: &crate::http::chrome_manifest::TlsSpec = &crate::http::chrome_manifest::CHROME_157.tls;

/// brotli (alg_id 2) certificate decompression for the clienthello
/// `compress_certificate` extension — Chrome advertises exactly this one.
/// Decompress-only: a client never compresses the server's certificate.
struct BrotliCertDecompress;

impl CertificateCompressor for BrotliCertDecompress {
    const ALGORITHM: CertificateCompressionAlgorithm = CertificateCompressionAlgorithm::BROTLI;
    const CAN_COMPRESS: bool = false;
    const CAN_DECOMPRESS: bool = true;

    fn decompress<W: io::Write>(&self, input: &[u8], output: &mut W) -> io::Result<()> {
        let mut d = brotli_decompressor::Decompressor::new(input, 4096);
        io::copy(&mut d, output)?;
        Ok(())
    }
}

/// Connector + verifier bundle for one `(profile, h1_only)` pair.
struct Endpoint {
    connector: SslConnector,
    verifier: Arc<LumenVerifier>,
}

type EndpointMap = HashMap<(TlsProfile, bool), Arc<Endpoint>>;

fn endpoint(profile: TlsProfile, h1_only: bool) -> Result<Arc<Endpoint>, Error> {
    static CACHE: OnceLock<Mutex<EndpointMap>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let key = (profile, h1_only);
    if let Some(e) = guard.get(&key) {
        return Ok(e.clone());
    }
    let e = Arc::new(build_endpoint(profile, h1_only)?);
    guard.insert(key, e.clone());
    Ok(e)
}

fn build_endpoint(profile: TlsProfile, h1_only: bool) -> Result<Endpoint, Error> {
    let mut b = SslConnector::builder(SslMethod::tls())
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("BoringSSL init: {e}")))?;
    // Ошибка здесь — на проводе дефолтные списки стека, то есть чужой JA4;
    // лучше отказ соединения, чем молчаливая подмена отпечатка.
    b.set_cipher_list(TLS.cipher_list)
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("BoringSSL cipher_list: {e}")))?;
    b.set_sigalgs_list(TLS.sigalgs)
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("BoringSSL sigalgs: {e}")))?;
    // Handshake-time chain validation stays OFF: `LumenVerifier` re-runs the
    // full policy (webpki + AIA + NUC + OCSP + bypass) on the DER chain after
    // the handshake — see module docs. `set_default_verify_paths` from
    // `SslConnector::builder` is likewise irrelevant under NONE.
    b.set_verify(SslVerifyMode::NONE);
    // TLS 1.3 session resumption, matching rustls' in-memory client cache —
    // a resumed session skips re-verification, exactly as before.
    b.set_session_cache_mode(boring::ssl::SslSessionCacheMode::CLIENT);

    // Chrome-grade ClientHello (BUG-ozon): BoringSSL defaults all of these
    // OFF (`ssl_lib.cc`: `grease_enabled(false)`, `permute_extensions(false)`,
    // `signed_cert_timestamps_enabled(false)`, `ocsp_stapling_enabled(false)`),
    // while real Chrome turns every one on. Without GREASE values inside the
    // cipher/extension/group/sigalg lists and a permuted extension order the
    // hello is a deterministic non-Chrome tell — Ozon's edge serves its hard
    // refuse page instead of the solvable challenge. Tor keeps BoringSSL
    // defaults: Tor Browser is Firefox-based and must not look like Chrome.
    if !matches!(profile, TlsProfile::Tor) {
        b.set_grease_enabled(true);
        b.set_permute_extensions(true);
        b.enable_signed_cert_timestamps();
        b.enable_ocsp_stapling();
        // compress_certificate (27): Chrome advertises brotli only. We can
        // decompress (brotli-decompressor is already a dep for HTTP bodies)
        // but never compress — servers only compress when asked to.
        let _ = b.add_certificate_compression_algorithm(BrotliCertDecompress);
        // trust_anchors (51764): an empty requested-anchors list still makes
        // BoringSSL emit the extension, like Chrome's populated one does.
        // SAFETY: `b` is a live SSL_CTX; null/0 is BoringSSL's documented
        // "empty requested-anchors list" input.
        unsafe {
            ffi::SSL_CTX_set1_requested_trust_anchors(b.as_ptr(), std::ptr::null(), 0);
        }
    }

    match profile {
        TlsProfile::Standard => {
            let _ = b.set_curves_list(TLS.groups);
            let _ = b.set_min_proto_version(Some(SslVersion::TLS1_2));
        }
        TlsProfile::Strict => {
            let _ = b.set_curves_list(TLS.groups);
            let _ = b.set_min_proto_version(Some(SslVersion::TLS1_3));
        }
        TlsProfile::Tor => {
            let _ = b.set_curves_list("X25519");
            let _ = b.set_min_proto_version(Some(SslVersion::TLS1_3));
        }
    }
    let alpn: &[u8] = match (profile, h1_only) {
        (TlsProfile::Tor, _) | (_, true) => TLS.alpn_h1,
        _ => TLS.alpn,
    };
    b.set_alpn_protos(alpn)
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("BoringSSL ALPN: {e}")))?;

    let verifier = LumenVerifier::new(
        super::trusted_root_store(),
        &Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
    )
    .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("TLS verifier: {e}")))?;

    Ok(Endpoint { connector: b.build(), verifier })
}

/// Perform the TLS handshake over `tcp` (already connected — direct, through
/// a SOCKS5 tunnel or an HTTP CONNECT tunnel) and verify the peer chain with
/// the same policy the rustls path applies.
///
/// Returns the established `SslStream`; the caller reads ALPN/cert info off
/// `stream.ssl()` and wraps it in `RawStream::Tls`.
pub(crate) fn connect(
    host: &str,
    tcp: TcpStream,
    profile: TlsProfile,
    h1_only: bool,
) -> Result<SslStream<TcpStream>, Error> {
    let ep = endpoint(profile, h1_only)?;
    let mut conf = ep
        .connector
        .configure()
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("TLS setup: {e}")))?;
    conf.set_verify_hostname(false);
    let mut ssl = conf
        .into_ssl(host)
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("TLS setup: {e}")))?;
    // Ask for the stapled OCSP response like rustls does — without it
    // `ocsp_status()` stays empty and the `Revoked` hard-fail can never fire.
    let _ = ssl.set_status_type(StatusType::OCSP);
    // ECH GREASE — Chrome always sends the outer `encrypted_client_hello`
    // extension with a GREASE'd payload even without real ECH keys
    // (`SSL_set_enable_ech_grease`). Tor skips it with the rest of the
    // Chrome-only ClientHello dressing.
    if !matches!(profile, TlsProfile::Tor) {
        ssl.set_enable_ech_grease(true);
        // server_padding (4832): Chrome sends the extension with a zero
        // requested size — BoringSSL only emits it when configured.
        // SAFETY: `ssl` is a live SSL; 0 is the documented "no padding
        // requested" value.
        unsafe {
            ffi::SSL_set_server_padding_request(ssl.as_ptr(), 0);
        }
        // ALPS (draft-vvv-tls-alps, codepoint 17613): Chrome carries its h2
        // SETTINGS payload inside the ClientHello. Without it the hello is
        // missing the strongest remaining Chrome-only extension (BUG-1835).
        // Only meaningful when "h2" is actually offered via ALPN.
        if !h1_only {
            // Wire-формат SETTINGS payload: (id:u16 BE, value:u32 BE) на
            // запись — те же значения, что `chrome_manifest::CHROME_157.h2`
            // шлёт первым SETTINGS-фреймом.
            const ALPS_H2_SETTINGS: [u8; 24] = [
                0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // HEADER_TABLE_SIZE = 65536
                0x00, 0x02, 0x00, 0x00, 0x00, 0x00, // ENABLE_PUSH = 0
                0x00, 0x04, 0x00, 0x60, 0x00, 0x00, // INITIAL_WINDOW_SIZE = 6291456
                0x00, 0x06, 0x00, 0x04, 0x00, 0x00, // MAX_HEADER_LIST_SIZE = 262144
            ];
            // SAFETY: `ssl` is a live SSL; `b"h2"` is NUL-free and `len`
            // matches its 2 bytes; the settings buffer outlives the call.
            unsafe {
                ffi::SSL_add_application_settings(
                    ssl.as_ptr(),
                    b"h2".as_ptr(),
                    2,
                    ALPS_H2_SETTINGS.as_ptr(),
                    ALPS_H2_SETTINGS.len(),
                );
            }
        }
    }

    let mut hs = ssl.setup_connect(tcp);
    let stream = loop {
        match hs.handshake() {
            Ok(s) => break s,
            Err(HandshakeError::WouldBlock(mid)) => hs = mid,
            Err(HandshakeError::Failure(mid)) => {
                return Err(handshake_failure(mid.into_error()));
            }
            Err(HandshakeError::SetupFailure(e)) => {
                return Err(net_failure(NetErrorCode::SslProtocolError, format!("TLS handshake: {e}")));
            }
        }
    };

    // Resumed sessions skip certificate verification — same rule rustls
    // follows (the first full handshake already vetted the chain).
    if !stream.ssl().session_reused() {
        verify_peer(stream.ssl(), host, &ep.verifier)?;
    }
    Ok(stream)
}

/// Post-handshake certificate policy: run [`LumenVerifier::verify_server_cert`]
/// on the DER chain BoringSSL collected. Every check the rustls path performs
/// — webpki path validation, AIA issuer completion, NUC consent, stapled-OCSP
/// verdict (incl. the `Revoked` hard-fail and `record_stapled` for the cert
/// panel), CT requirements and the bypass override — lives inside that one
/// call.
fn verify_peer(ssl: &boring::ssl::SslRef, host: &str, verifier: &LumenVerifier) -> Result<(), Error> {
    let leaf_x509 = ssl
        .peer_certificate()
        .ok_or_else(|| net_failure(NetErrorCode::SslProtocolError, "TLS: no peer certificate".to_owned()))?;
    let leaf_der = leaf_x509
        .to_der()
        .map_err(|e| net_failure(NetErrorCode::SslProtocolError, format!("TLS peer cert: {e}")))?;
    let leaf = CertificateDer::from(leaf_der);

    let mut intermediates: Vec<CertificateDer<'static>> = Vec::new();
    if let Some(chain) = ssl.peer_cert_chain() {
        for cert in chain.iter() {
            if let Ok(der) = cert.to_der() {
                let der = CertificateDer::from(der);
                // `peer_cert_chain` may or may not include the leaf depending
                // on how the peer framed the flight — keep intermediates only.
                if der.as_ref() != leaf.as_ref() {
                    intermediates.push(der);
                }
            }
        }
    }

    let ocsp_response = ssl.ocsp_status().unwrap_or(&[]);
    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|e| net_failure(NetErrorCode::InvalidUrl, format!("invalid hostname '{host}': {e}")))?;
    let now = UnixTime::now();

    match verifier.verify_server_cert(&leaf, &intermediates, &server_name, ocsp_response, now) {
        Ok(_) => Ok(()),
        Err(rustls_err) => Err(cert_verify_error(rustls_err)),
    }
}

/// Map the `rustls::Error` [`LumenVerifier`] returns onto the same
/// [`Error::CertInvalid`] shape `handshake_io_error` produces for the rustls
/// transport, so `is_transient_handshake_error`/the cert interstitial treat
/// both backends identically.
fn cert_verify_error(rustls_err: rustls::Error) -> Error {
    // `cert_error::from_io_error` downcasts through `io::Error::get_ref`;
    // wrapping the rustls error reproduces exactly what
    // `complete_io`'s `InvalidData` wrapper looked like.
    let wrapped = io::Error::new(io::ErrorKind::InvalidData, rustls_err);
    match cert_error::from_io_error(&wrapped) {
        Some(cert_err) => Error::CertInvalid(cert_err),
        None => net_failure(NetErrorCode::SslProtocolError, format!("TLS verify: {wrapped}")),
    }
}

fn handshake_failure(err: boring::ssl::Error) -> Error {
    // A boring handshake error can carry an io cause (timeout, reset); keep
    // the same message shape `handshake_io_error` used so transient-error
    // classification keeps working. The io error is rebuilt rather than
    // cloned — `cert_error::from_io_error`'s rustls downcast can never match
    // a boring error anyway.
    if let Some(io_err) = err.io_error() {
        let rebuilt = io::Error::new(io_err.kind(), io_err.to_string());
        return crate::handshake_io_error("TLS handshake", rebuilt);
    }
    net_failure(NetErrorCode::SslProtocolError, format!("TLS handshake: {err}"))
}

/// `CertInfo` for the cert panel, built from the BoringSSL peer chain —
/// mirrors `cert_info_from_completed_handshake` in `lib.rs`.
pub(crate) fn cert_info(stream: &SslStream<TcpStream>, host: &str) -> Option<super::CertInfo> {
    let leaf = stream.ssl().peer_certificate()?;
    let leaf_der = CertificateDer::from(leaf.to_der().ok()?);

    let mut chain: Vec<CertificateDer<'static>> = vec![leaf_der.clone()];
    if let Some(certs) = stream.ssl().peer_cert_chain() {
        for cert in certs.iter() {
            if let Ok(der) = cert.to_der() {
                let der = CertificateDer::from(der);
                if der.as_ref() != leaf_der.as_ref() {
                    chain.push(der);
                }
            }
        }
    }

    let tls_version = match stream.ssl().version2() {
        Some(SslVersion::TLS1_2) => "TLS 1.2",
        Some(SslVersion::TLS1_3) => "TLS 1.3",
        _ => "TLS",
    }
    .to_owned();

    let ct_verdict = ct::evaluate_ct(leaf_der.as_ref(), chain.get(1).map(AsRef::as_ref));
    let stapled = ocsp::stapled_verdict(host);
    let mut info = super::CertInfo::from_peer_cert(
        leaf_der.as_ref(),
        &tls_version,
        stapled.unwrap_or(ocsp::OcspVerdict::Unknown),
        ct_verdict,
    );
    if stapled.is_none() {
        info.revocation_status =
            String::from("Not checked (no certificate verification in this session)");
    }
    info.local_root = super::os_roots::local_root_name(&chain).unwrap_or_default();
    if nuc::chain_names_root(&chain) && nuc::has_consent(host) {
        info.gov_ca_scts = Some(ct_verdict.operators());
    }
    Some(info)
}

/// Negotiated ALPN as owned bytes (borrowed from the stream otherwise).
pub(crate) fn negotiated_alpn(stream: &SslStream<TcpStream>) -> Option<Vec<u8>> {
    stream.ssl().selected_alpn_protocol().map(<[u8]>::to_vec)
}
