//! Категории ошибок, общие для всего проекта. Каждый модуль может оборачивать
//! свои частные ошибки в подходящий вариант через `From`.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Network(String),
    Parse(String),
    Io(String),
    Storage(String),
    InvalidUrl(String),
    PermissionDenied(String),
    NotFound(String),
    Other(String),
    /// Operation was cooperatively cancelled (e.g. an in-flight `fetch()` aborted
    /// via an `AbortSignal`). The JS layer maps this to a DOMException `AbortError`.
    Aborted(String),
    /// GAP-CSPENF: a `fetch()`/`XMLHttpRequest` request was blocked before any
    /// network I/O by the document's CSP `connect-src` (or `default-src`).
    /// Raised by `lumen-network::HttpClient`, which has no JS runtime to fire
    /// `securitypolicyviolation` itself — the native fetch binding in
    /// `lumen-js` matches on this variant and dispatches from there, using
    /// `blocked_uri`/`original_policy` as `SecurityPolicyViolationEvent`'s
    /// `blockedURI`/`originalPolicy` (CSP3 §7.8).
    CspConnectSrcBlocked {
        blocked_uri: String,
        original_policy: String,
    },
    /// GAP-CSPENF срез 13: a `new Worker(url)`/`new SharedWorker(url)` classic
    /// script fetch was blocked before any network I/O by the document's CSP
    /// `worker-src` (falling back to `default-src`). Raised by
    /// `lumen-network::HttpClient::check_worker_src`, mirroring
    /// [`Self::CspConnectSrcBlocked`] — the native worker-creation binding in
    /// `lumen-js` matches on this variant and dispatches
    /// `securitypolicyviolation` from there.
    CspWorkerSrcBlocked {
        blocked_uri: String,
        original_policy: String,
    },
    /// GAP-CSPENF срез 16: an `<embed src>`/`<object data>` resource fetch was
    /// blocked before any network I/O by the document's CSP `object-src`
    /// (falling back to `default-src`). Raised by
    /// `lumen-network::HttpClient::check_object_src`, mirroring
    /// [`Self::CspWorkerSrcBlocked`] — the native embed/object fetch-check
    /// binding in `lumen-js` matches on this variant and dispatches
    /// `securitypolicyviolation` from there.
    CspObjectSrcBlocked {
        blocked_uri: String,
        original_policy: String,
    },
    /// GAP-CSPENF срез 17: a `<video src>`/`<audio src>`/`<track src>` resource
    /// fetch was blocked before any network I/O by the document's CSP
    /// `media-src` (falling back to `default-src`). Raised by
    /// `lumen-network::HttpClient::check_media_src`, mirroring
    /// [`Self::CspObjectSrcBlocked`] — the native media fetch-check binding in
    /// `lumen-js` matches on this variant and dispatches
    /// `securitypolicyviolation` from there.
    CspMediaSrcBlocked {
        blocked_uri: String,
        original_policy: String,
    },
    /// ph3-tls-hardening (A1): the TLS handshake completed but the peer's
    /// certificate failed trust verification. Distinct from [`Self::Network`]
    /// so the shell can route this to a cert interstitial ("your connection
    /// is not private") instead of the generic error page — see
    /// `crates/shell/src/panels/cert_panel.rs`. Raised by
    /// `lumen-network::tls::cert_error::from_io_error`, which downcasts the
    /// `rustls::Error` that `ClientConnection::complete_io` wraps in an
    /// `io::Error` (`rustls` itself has no `lumen-core` dependency, so the
    /// mapping lives on the network side; this variant stays TLS-library
    /// agnostic).
    CertInvalid(CertError),
}

/// Structured reason a TLS certificate failed verification — the payload of
/// [`Error::CertInvalid`]. Kept free of any TLS-library type (`rustls` is not
/// a `lumen-core` dependency) so `lumen-shell` can match on it without
/// depending on `lumen-network`'s TLS internals.
///
/// OCSP/CT-specific variants ([`Self::Revoked`], [`Self::CtInsufficient`])
/// are defined now (ph3-tls-hardening A1) but not yet raised anywhere —
/// stapled-OCSP parsing (A3) and CT-log validation (A4) are later slices of
/// the same task and will start returning them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertError {
    /// The chain does not lead to a trusted root, or a signature in the
    /// chain does not verify (rustls `UnknownIssuer`/`BadSignature`/...).
    /// Carries rustls's own description for the cert panel's reason line.
    Untrusted(String),
    /// The leaf or an intermediate certificate is past its `notAfter`.
    Expired,
    /// The certificate has been revoked (stapled OCSP `certStatus: revoked`).
    Revoked,
    /// Fewer than the policy-required number of valid SCTs from distinct CT
    /// logs were presented.
    CtInsufficient,
    /// The presented certificate's SANs do not cover the requested hostname.
    NameMismatch,
    /// The leaf certificate is self-signed.
    SelfSigned,
    /// Any other structural cert failure (bad encoding, unhandled critical
    /// extension, unsupported signature algorithm, ...) — carries rustls's
    /// own description.
    Other(String),
}

impl fmt::Display for CertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Untrusted(reason) => write!(f, "untrusted certificate chain: {reason}"),
            Self::Expired => write!(f, "certificate expired"),
            Self::Revoked => write!(f, "certificate revoked"),
            Self::CtInsufficient => write!(f, "insufficient certificate transparency evidence"),
            Self::NameMismatch => write!(f, "certificate not valid for this hostname"),
            Self::SelfSigned => write!(f, "self-signed certificate"),
            Self::Other(reason) => write!(f, "invalid certificate: {reason}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(s) => write!(f, "network error: {s}"),
            Self::Parse(s) => write!(f, "parse error: {s}"),
            Self::Io(s) => write!(f, "io error: {s}"),
            Self::Storage(s) => write!(f, "storage error: {s}"),
            Self::InvalidUrl(s) => write!(f, "invalid url: {s}"),
            Self::PermissionDenied(s) => write!(f, "permission denied: {s}"),
            Self::NotFound(s) => write!(f, "not found: {s}"),
            Self::Other(s) => write!(f, "{s}"),
            Self::Aborted(s) => write!(f, "aborted: {s}"),
            Self::CspConnectSrcBlocked { blocked_uri, original_policy } => {
                write!(f, "connect-src blocked '{blocked_uri}' per policy \"{original_policy}\"")
            }
            Self::CspWorkerSrcBlocked { blocked_uri, original_policy } => {
                write!(f, "worker-src blocked '{blocked_uri}' per policy \"{original_policy}\"")
            }
            Self::CspObjectSrcBlocked { blocked_uri, original_policy } => {
                write!(f, "object-src blocked '{blocked_uri}' per policy \"{original_policy}\"")
            }
            Self::CspMediaSrcBlocked { blocked_uri, original_policy } => {
                write!(f, "media-src blocked '{blocked_uri}' per policy \"{original_policy}\"")
            }
            Self::CertInvalid(cert_err) => write!(f, "TLS handshake: {cert_err}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
