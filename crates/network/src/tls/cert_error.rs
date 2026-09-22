//! Map `rustls`'s handshake failure into the TLS-library-agnostic
//! [`lumen_core::error::CertError`] (ph3-tls-hardening, part A1).
//!
//! `rustls::ClientConnection::complete_io` wraps every `process_new_packets`
//! failure as `io::Error::new(io::ErrorKind::InvalidData, rustls_error)`
//! (`rustls::conn::ConnectionCommon::complete_io`) — the original
//! `rustls::Error` survives behind `io::Error::get_ref()`, downcastable back
//! out. This module is the single place that performs that downcast, so the
//! handshake call sites in `lib.rs` stay free of `rustls::Error` matching.

use lumen_core::error::CertError;

/// Extract a [`CertError`] from an `io::Error` produced by
/// [`rustls::ClientConnection::complete_io`]. Returns `None` for every
/// handshake failure that is not a certificate-trust problem (corrupt
/// message, decrypt failure, plain I/O error, ...) — those stay
/// `Error::Network` at the call site, which keeps them eligible for the
/// existing transient-handshake-error retry.
pub fn from_io_error(err: &std::io::Error) -> Option<CertError> {
    let rustls_err = err.get_ref()?.downcast_ref::<rustls::Error>()?;
    let rustls::Error::InvalidCertificate(cert_err) = rustls_err else {
        return None;
    };
    Some(map_certificate_error(cert_err))
}

/// Map a single `rustls::CertificateError` onto the [`CertError`] the shell
/// understands. `CertificateError` is `#[non_exhaustive]` (rustls can add
/// variants without a major bump), so anything not named explicitly below
/// folds into [`CertError::Other`] with rustls's own `Debug` description —
/// still surfaced to the user, just without a dedicated UI treatment.
fn map_certificate_error(err: &rustls::CertificateError) -> CertError {
    use rustls::CertificateError as CE;
    match err {
        CE::Expired | CE::ExpiredContext { .. } => CertError::Expired,
        CE::Revoked => CertError::Revoked,
        CE::NotValidForName | CE::NotValidForNameContext { .. } => CertError::NameMismatch,
        // webpki reports a self-signed leaf as "issuer not found" too, same
        // as any other untrusted chain — rustls has no distinct
        // `CertificateError::SelfSigned`. `CertError::SelfSigned` stays
        // reserved for A5's leaf-cert parsing (issuer == subject check).
        CE::UnknownIssuer | CE::BadSignature => CertError::Untrusted(format!("{err:?}")),
        other => CertError::Other(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io_error_from(rustls_err: rustls::Error) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, rustls_err)
    }

    #[test]
    fn expired_maps_to_expired() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::Expired,
        ));
        assert_eq!(from_io_error(&err), Some(CertError::Expired));
    }

    #[test]
    fn unknown_issuer_maps_to_untrusted() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));
        assert!(matches!(from_io_error(&err), Some(CertError::Untrusted(_))));
    }

    #[test]
    fn bad_signature_maps_to_untrusted() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::BadSignature,
        ));
        assert!(matches!(from_io_error(&err), Some(CertError::Untrusted(_))));
    }

    #[test]
    fn revoked_maps_to_revoked() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::Revoked,
        ));
        assert_eq!(from_io_error(&err), Some(CertError::Revoked));
    }

    #[test]
    fn name_mismatch_maps_to_name_mismatch() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidForName,
        ));
        assert_eq!(from_io_error(&err), Some(CertError::NameMismatch));
    }

    #[test]
    fn unhandled_critical_extension_falls_back_to_other() {
        let err = io_error_from(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnhandledCriticalExtension,
        ));
        assert!(matches!(from_io_error(&err), Some(CertError::Other(_))));
    }

    #[test]
    fn non_certificate_rustls_error_returns_none() {
        let err = io_error_from(rustls::Error::DecryptError);
        assert_eq!(from_io_error(&err), None);
    }

    #[test]
    fn plain_io_error_without_rustls_source_returns_none() {
        let err = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof");
        assert_eq!(from_io_error(&err), None);
    }
}
