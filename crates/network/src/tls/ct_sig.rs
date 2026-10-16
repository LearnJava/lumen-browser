//! SCT signature check over a pre-certificate entry (RFC 6962 §3.2) — COMPAT-TRUST-CTLOGS.
//!
//! A CA logs a *pre-certificate* (the final certificate plus a poison extension) and embeds
//! the returned SCTs into the final certificate. So the bytes a log signed are not the
//! certificate the browser sees: they are rebuilt from it by [`precert_tbs`] — the leaf's
//! `TBSCertificate` with the SCT-list extension cut out, every other byte untouched — plus
//! the SHA-256 of the issuer's `SubjectPublicKeyInfo`. [`verify`] then checks the log's
//! ECDSA P-256/SHA-256 signature (the only algorithm in the bundled log list) over:
//!
//! ```text
//! sct_version(1) | signature_type = 0 (1) | timestamp(8) | entry_type = 1 (2)
//!   | issuer_key_hash(32) | tbs_certificate(u24 length + bytes) | extensions(u16 length + bytes)
//! ```
//!
//! Not handled, and reported as "does not verify" (the SCT simply does not count): a leaf
//! issued by a dedicated *precertificate signing* CA (RFC 6962 §3.1 — the issuer name and key
//! hash then refer to the real CA), RSA logs, and SCT versions other than v1.

use super::ct::{Der, Sct, OID_CT_SCT_LIST, TAG_CONTEXT_0, TAG_CONTEXT_3, TAG_INTEGER, TAG_OID, TAG_SEQUENCE};
use super::ct_logs::CtLog;
use crate::h3::tls_cert_verify::ecdsa_p256_sha256_verify;
use crate::h3::x509_spki::{parse_subject_public_key_info, SpkiAlgorithm};

/// `SignatureAndHashAlgorithm.hash` = sha256 (RFC 5246 §7.4.1.4.1).
const HASH_SHA256: u8 = 4;
/// `SignatureAndHashAlgorithm.signature` = ecdsa.
const SIGNATURE_ECDSA: u8 = 3;
/// `SignatureType` = `certificate_timestamp` (RFC 6962 §3.2).
const SIGNATURE_TYPE_CERTIFICATE_TIMESTAMP: u8 = 0;
/// `LogEntryType` = `precert_entry`.
const ENTRY_TYPE_PRECERT: u16 = 1;

/// DER-encode one TLV: tag, definite length, contents.
fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let len = contents.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes = len.to_be_bytes();
        let skip = bytes.iter().take_while(|&&b| b == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(contents);
    out
}

/// Walk a certificate's `TBSCertificate` and hand back its contents.
fn tbs_contents(cert_der: &[u8]) -> Option<&[u8]> {
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE)?;
    Der::new(certificate).read_tagged(TAG_SEQUENCE)
}

/// The whole `subjectPublicKeyInfo` TLV of a certificate — the exact bytes whose SHA-256 is
/// the `issuer_key_hash` of a pre-certificate entry (RFC 6962 §3.2).
pub(super) fn subject_public_key_info(cert_der: &[u8]) -> Option<&[u8]> {
    let mut tbs = Der::new(tbs_contents(cert_der)?);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version
    }
    tbs.read_tagged(TAG_INTEGER)?; // serialNumber
    for _ in 0..4 {
        tbs.read_tagged(TAG_SEQUENCE)?; // signature, issuer, validity, subject
    }
    let (tag, _, raw) = tbs.read_raw()?;
    (tag == TAG_SEQUENCE).then_some(raw)
}

/// The leaf's `TBSCertificate` as the CA logged it: the same DER with the SCT-list
/// extension removed. `None` if the certificate is malformed or carries no extensions.
pub(super) fn precert_tbs(cert_der: &[u8]) -> Option<Vec<u8>> {
    let mut tbs = Der::new(tbs_contents(cert_der)?);
    let mut rebuilt = Vec::new();
    while let Some(tag) = tbs.peek_tag() {
        let (_, contents, raw) = tbs.read_raw()?;
        if tag != TAG_CONTEXT_3 {
            rebuilt.extend_from_slice(raw);
            continue;
        }
        // extensions [3] EXPLICIT SEQUENCE OF Extension — copy all but the SCT list.
        let mut extensions = Der::new(Der::new(contents).read_tagged(TAG_SEQUENCE)?);
        let mut kept = Vec::new();
        while !extensions.is_empty() {
            let (_, extension, raw) = extensions.read_raw()?;
            let mut fields = Der::new(extension);
            if fields.read_tagged(TAG_OID)? != OID_CT_SCT_LIST {
                kept.extend_from_slice(raw);
            }
        }
        rebuilt.extend_from_slice(&tlv(TAG_CONTEXT_3, &tlv(TAG_SEQUENCE, &kept)));
    }
    Some(tlv(TAG_SEQUENCE, &rebuilt))
}

/// The `digitally-signed` input of a pre-certificate SCT (RFC 6962 §3.2).
fn signed_data(sct: &Sct<'_>, issuer_key_hash: &[u8; 32], tbs: &[u8]) -> Option<Vec<u8>> {
    let tbs_len = u32::try_from(tbs.len()).ok().filter(|&n| n < 1 << 24)?;
    let ext_len = u16::try_from(sct.extensions.len()).ok()?;
    let mut data = Vec::with_capacity(tbs.len() + sct.extensions.len() + 56);
    data.push(sct.version);
    data.push(SIGNATURE_TYPE_CERTIFICATE_TIMESTAMP);
    data.extend_from_slice(&sct.timestamp_ms.to_be_bytes());
    data.extend_from_slice(&ENTRY_TYPE_PRECERT.to_be_bytes());
    data.extend_from_slice(issuer_key_hash);
    data.extend_from_slice(&tbs_len.to_be_bytes()[1..]);
    data.extend_from_slice(tbs);
    data.extend_from_slice(&ext_len.to_be_bytes());
    data.extend_from_slice(sct.extensions);
    Some(data)
}

/// Whether `sct` carries a valid signature by `log` over the pre-certificate entry made of
/// `tbs` (from [`precert_tbs`]) and `issuer_key_hash`.
pub(super) fn verify(sct: &Sct<'_>, log: &CtLog, issuer_key_hash: &[u8; 32], tbs: &[u8]) -> bool {
    if sct.version != 0 || sct.hash_algorithm != HASH_SHA256 || sct.signature_algorithm != SIGNATURE_ECDSA {
        return false;
    }
    let Ok(key) = parse_subject_public_key_info(&log.spki) else { return false };
    if key.algorithm != SpkiAlgorithm::EcdsaP256 {
        return false;
    }
    let Some(data) = signed_data(sct, issuer_key_hash, tbs) else { return false };
    ecdsa_p256_sha256_verify(&key.key_material, &data, sct.signature).is_ok()
}
