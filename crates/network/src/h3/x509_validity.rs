//! X.509 certificate validity-period verification (RFC 5280 §4.1.2.5) — slice 64 of
//! the HTTP/3 sprint.
//!
//! Two sibling slices authenticate the server's end-entity certificate but each
//! deliberately leaves the *time* dimension open. The possession check
//! ([`conn_cert_auth`](super::conn_cert_auth)) notes it "does not \[…\] honour
//! `notBefore`/`notAfter` validity dates", and the hostname check
//! ([`x509_hostname`](super::x509_hostname)) likewise defers "honouring
//! `notBefore`/`notAfter`". This module is that missing check: it answers "is this
//! certificate valid *now*?" — the third leg of "should this certificate be trusted
//! for this origin?", after possession (does the peer hold the key?) and identity
//! (does the certificate name the host?).
//!
//! ## What it reads
//!
//! The `validity` field of the `TBSCertificate` (RFC 5280 §4.1.2.5):
//!
//! ```text
//! Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
//! TBSCertificate ::= SEQUENCE {
//!     version      [0] EXPLICIT ... DEFAULT v1,   -- context 0xA0, optional
//!     serialNumber     INTEGER,
//!     signature        AlgorithmIdentifier,
//!     issuer           Name,
//!     validity         Validity,                  -- <- the target
//!     ... }
//!
//! Validity ::= SEQUENCE { notBefore Time, notAfter Time }
//! Time     ::= CHOICE {
//!     utcTime          UTCTime,           -- tag 0x17, "YYMMDDHHMMSSZ"
//!     generalTime      GeneralizedTime }  -- tag 0x18, "YYYYMMDDHHMMSSZ"
//! ```
//!
//! [`verify_certificate_validity`] navigates the `TBSCertificate` by field order to
//! the `validity` SEQUENCE, decodes its two `Time` values into absolute instants
//! (seconds since the Unix epoch), and checks that a caller-supplied *now* falls
//! within `[notBefore, notAfter]`.
//!
//! ## Time encoding
//!
//! RFC 5280 §4.1.2.5 constrains both forms to UTC with a mandatory seconds field and
//! a `Z` suffix — no fractional seconds, no timezone offset:
//!
//! - **`UTCTime`** (through 2049): `YYMMDDHHMMSSZ`. The two-digit year is interpreted
//!   per RFC 5280 §4.1.2.5.1 — `50..=99` is `19YY`, `00..=49` is `20YY`.
//! - **`GeneralizedTime`** (2050 onward): `YYYYMMDDHHMMSSZ`, a full four-digit year.
//!
//! The civil date is converted to a day count with the standard
//! [`days_from_civil`](https://howardhinnant.github.io/date_algorithms.html)
//! algorithm (valid across the proleptic Gregorian calendar) and combined with the
//! time of day — a pure integer computation, no calendar dependency.
//!
//! ## Purity
//!
//! A pure function over borrowed certificate bytes and a caller-supplied timestamp:
//! it takes *now* as a parameter (seconds since the Unix epoch) rather than reading a
//! clock, so it stays deterministic and testable, exactly as
//! [`x509_spki`](super::x509_spki) and [`x509_hostname`](super::x509_hostname) are.
//! The real wall-clock time is threaded in from the connect loop
//! ([`conn_connect`](super::conn_connect)), which judges the end-entity certificate's
//! validity period against a caller-supplied `now_unix` after possession and identity,
//! as slice 63 did for the hostname verifier.
//!
//! ## The chain walk (RFC 5280 §6.1.3(a)(2))
//!
//! [`verify_certificate_validity`] checks one certificate; [`verify_validity_chain`]
//! (slice 78) applies it to *every* certificate in the server-presented
//! `certificate_list`. RFC 5280 §6.1.3(a)(2) requires each certificate in the path to be
//! within its validity period, not just the leaf — an expired intermediate can no longer
//! be trusted to have issued the certificate below it. This is the validity companion of
//! the signature ([`x509_chain`](super::x509_chain)), name
//! ([`x509_name_chain`](super::x509_name_chain)), `basicConstraints`
//! ([`x509_basic_constraints`](super::x509_basic_constraints)), and `keyUsage`
//! ([`x509_key_usage`](super::x509_key_usage)) walks — but unlike those issuer-only walks
//! it covers the leaf too. Wiring it into the connect loop beside the single-certificate
//! end-entity check is a later slice.

/// The DER tag for `SEQUENCE` (and `SEQUENCE OF`), constructed universal.
const TAG_SEQUENCE: u8 = 0x30;
/// The DER tag for `INTEGER`.
const TAG_INTEGER: u8 = 0x02;
/// The DER tag for the `[0] EXPLICIT` `version` field of a `TBSCertificate`.
const TAG_CONTEXT_0: u8 = 0xA0;
/// The DER tag for `UTCTime` — `YYMMDDHHMMSSZ` (RFC 5280 §4.1.2.5.1).
const TAG_UTC_TIME: u8 = 0x17;
/// The DER tag for `GeneralizedTime` — `YYYYMMDDHHMMSSZ` (RFC 5280 §4.1.2.5.2).
const TAG_GENERALIZED_TIME: u8 = 0x18;

/// Why verifying a certificate's validity period failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidityError {
    /// The certificate DER was truncated, mis-nested, or carried an unexpected tag
    /// where the structure required a specific one. Carries a static hint naming the
    /// field that did not decode.
    Malformed(&'static str),
    /// A `Time` value did not match the RFC 5280 profile: a tag other than `UTCTime`
    /// or `GeneralizedTime`, a wrong length, a non-`Z` suffix, a non-digit octet, or a
    /// field out of range (e.g. month 13). Carries a static hint.
    MalformedTime(&'static str),
    /// The certificate's `notBefore` is in the future relative to *now*: the
    /// certificate is not yet valid (RFC 5280 §4.1.2.5). A fatal authentication
    /// failure: the connection must not carry application data.
    NotYetValid {
        /// The certificate's `notBefore`, seconds since the Unix epoch.
        not_before: i64,
        /// The *now* the check was made against, seconds since the Unix epoch.
        now: i64,
    },
    /// The certificate's `notAfter` is in the past relative to *now*: the certificate
    /// has expired (RFC 5280 §4.1.2.5). A fatal authentication failure.
    Expired {
        /// The certificate's `notAfter`, seconds since the Unix epoch.
        not_after: i64,
        /// The *now* the check was made against, seconds since the Unix epoch.
        now: i64,
    },
}

impl core::fmt::Display for ValidityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed certificate: {what}"),
            Self::MalformedTime(what) => write!(f, "malformed certificate time: {what}"),
            Self::NotYetValid { not_before, now } => {
                write!(f, "certificate not yet valid (notBefore {not_before} > now {now})")
            }
            Self::Expired { not_after, now } => {
                write!(f, "certificate expired (notAfter {not_after} < now {now})")
            }
        }
    }
}

impl std::error::Error for ValidityError {}

/// A minimal reader over a DER-encoded byte slice, walking tag-length-value triples
/// left to right. Definite-length only (DER forbids the indefinite form). A sibling of
/// [`x509_spki`](super::x509_spki)'s and [`x509_hostname`](super::x509_hostname)'s
/// readers, specialised to this slice's error type.
struct Der<'a> {
    /// The bytes being read.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Der<'a> {
    /// A reader positioned at the start of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// The number of unread bytes.
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// The tag of the next TLV without consuming it, or `None` at end of input.
    fn peek_tag(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Read a DER definite length at the cursor (X.690 DER): a short form
    /// (`0x00..=0x7f`) is the length itself; a long form (`0x81..`) gives the count of
    /// big-endian length octets that follow. The indefinite form (`0x80`) and counts
    /// wider than four octets are rejected.
    fn read_length(&mut self) -> Result<usize, ValidityError> {
        let first = *self
            .bytes
            .get(self.pos)
            .ok_or(ValidityError::Malformed("truncated length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(ValidityError::Malformed("unsupported DER length form"));
        }
        if self.remaining() < count {
            return Err(ValidityError::Malformed("truncated long-form length"));
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = (len << 8) | self.bytes[self.pos] as usize;
            self.pos += 1;
        }
        Ok(len)
    }

    /// Read one TLV, returning its tag and a slice over its contents, and advance the
    /// cursor past it.
    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), ValidityError> {
        let tag = *self
            .bytes
            .get(self.pos)
            .ok_or(ValidityError::Malformed("truncated: expected a tag"))?;
        self.pos += 1;
        let len = self.read_length()?;
        if self.remaining() < len {
            return Err(ValidityError::Malformed("truncated: content shorter than its length"));
        }
        let contents = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok((tag, contents))
    }

    /// Read one TLV and require it to carry `tag`, returning its contents. `what` names
    /// the field for the error if the tag does not match.
    fn read_tagged(&mut self, tag: u8, what: &'static str) -> Result<&'a [u8], ValidityError> {
        let (t, contents) = self.read_tlv()?;
        if t != tag {
            return Err(ValidityError::Malformed(what));
        }
        Ok(contents)
    }
}

/// Verify that an end-entity certificate is valid at `now` (RFC 5280 §4.1.2.5).
///
/// `cert_der` is one X.509 certificate — the `cert_data` of the first
/// [`CertificateEntry`](super::tls_message::CertificateEntry) of the server's
/// `Certificate` message, the end-entity certificate (RFC 8446 §4.4.2). `now` is the
/// current time as seconds since the Unix epoch (1970-01-01T00:00:00Z), supplied by
/// the caller so the check stays clock-free.
///
/// The certificate is valid when `notBefore <= now <= notAfter`. On success it says
/// nothing about the certificate's chain to a trust anchor, its possession proof, or
/// its hostname coverage — those are the sibling checks
/// ([`conn_cert_auth`](super::conn_cert_auth), [`x509_hostname`](super::x509_hostname),
/// and a later trust-anchor slice).
///
/// # Errors
///
/// - [`ValidityError::Malformed`] if the certificate DER is truncated or mis-structured.
/// - [`ValidityError::MalformedTime`] if a `Time` value violates the RFC 5280 profile.
/// - [`ValidityError::NotYetValid`] if `now < notBefore`.
/// - [`ValidityError::Expired`] if `now > notAfter`.
pub fn verify_certificate_validity(cert_der: &[u8], now: i64) -> Result<(), ValidityError> {
    let (not_before, not_after) = certificate_validity(cert_der)?;
    if now < not_before {
        return Err(ValidityError::NotYetValid { not_before, now });
    }
    if now > not_after {
        return Err(ValidityError::Expired { not_after, now });
    }
    Ok(())
}

/// Decode a certificate's `notBefore` and `notAfter` as seconds since the Unix epoch
/// (RFC 5280 §4.1.2.5), without checking them against any clock.
///
/// Exposed alongside [`verify_certificate_validity`] so a caller that already knows
/// *now* — or that wants to report the window — can read the raw bounds. The returned
/// pair is `(notBefore, notAfter)`.
///
/// # Errors
///
/// [`ValidityError::Malformed`] if the DER does not decode to the `validity` SEQUENCE,
/// or [`ValidityError::MalformedTime`] if either `Time` violates the RFC 5280 profile.
pub fn certificate_validity(cert_der: &[u8]) -> Result<(i64, i64), ValidityError> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }.
    let certificate = Der::new(cert_der).read_tagged(TAG_SEQUENCE, "certificate is not a SEQUENCE")?;
    let tbs = Der::new(certificate).read_tagged(TAG_SEQUENCE, "tbsCertificate is not a SEQUENCE")?;

    // TBSCertificate fields in order, up to validity.
    let mut tbs = Der::new(tbs);
    if tbs.peek_tag() == Some(TAG_CONTEXT_0) {
        tbs.read_tlv()?; // version [0] EXPLICIT — optional (absent = v1)
    }
    tbs.read_tagged(TAG_INTEGER, "serialNumber is not an INTEGER")?;
    tbs.read_tagged(TAG_SEQUENCE, "signature AlgorithmIdentifier is not a SEQUENCE")?;
    tbs.read_tagged(TAG_SEQUENCE, "issuer is not a SEQUENCE")?;
    let validity = tbs.read_tagged(TAG_SEQUENCE, "validity is not a SEQUENCE")?;

    // Validity ::= SEQUENCE { notBefore Time, notAfter Time }.
    let mut validity = Der::new(validity);
    let (not_before_tag, not_before_bytes) = validity.read_tlv()?;
    let not_before = parse_time(not_before_tag, not_before_bytes)?;
    let (not_after_tag, not_after_bytes) = validity.read_tlv()?;
    let not_after = parse_time(not_after_tag, not_after_bytes)?;

    Ok((not_before, not_after))
}

/// Why verifying an entire certificate chain's validity periods failed (RFC 5280
/// §6.1.3(a)(2)). A sibling of the issuer-only walk errors
/// [`x509_chain::ChainWalkError`](super::x509_chain::ChainWalkError),
/// [`x509_name_chain::NameChainWalkError`](super::x509_name_chain::NameChainWalkError),
/// [`x509_basic_constraints::CaConstraintsWalkError`](super::x509_basic_constraints::CaConstraintsWalkError),
/// and [`x509_key_usage::KeyUsageWalkError`](super::x509_key_usage::KeyUsageWalkError).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidityWalkError {
    /// The certificate list was empty. RFC 8446 §4.4.2 requires the end-entity
    /// certificate first, so there is nothing to walk — a malformed `Certificate`
    /// message.
    EmptyChain,
    /// The certificate at `index` failed its validity check: its DER did not decode to
    /// a `validity` SEQUENCE, a `Time` value violated the RFC 5280 profile, or the
    /// certificate was outside its `[notBefore, notAfter]` window at *now*.
    Certificate {
        /// Position, in the `certificate_list`, of the certificate that failed.
        index: usize,
        /// The underlying single-certificate validity failure.
        error: ValidityError,
    },
}

impl core::fmt::Display for ValidityWalkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyChain => f.write_str("certificate chain is empty"),
            Self::Certificate { index, error } => write!(f, "certificate #{index}: {error}"),
        }
    }
}

impl std::error::Error for ValidityWalkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Certificate { error, .. } => Some(error),
            Self::EmptyChain => None,
        }
    }
}

/// Verify that *every* certificate in a server-presented chain is within its validity
/// period at `now` (RFC 5280 §6.1.3(a)(2), RFC 8446 §4.4.2).
///
/// [`verify_certificate_validity`] checks a single certificate — the connect loop calls
/// it on the end-entity leaf after possession and identity. But RFC 5280 §6.1.3(a)(2)
/// requires the check for *each* certificate in the certification path: an expired
/// intermediate is as fatal as an expired leaf, since a certificate outside its validity
/// period can no longer be trusted to have issued the one below it. This walk is the
/// validity companion of the signature walk
/// ([`x509_chain::verify_chain_signatures`](super::x509_chain::verify_chain_signatures)),
/// the name walk ([`x509_name_chain::verify_name_chain`](super::x509_name_chain::verify_name_chain)),
/// and the `basicConstraints` / `keyUsage` walks — layered beside them over the same
/// presented list.
///
/// `chain` is the ordered `certificate_list` from the server's `Certificate` message —
/// the end-entity certificate first, then each issuing certificate. Unlike the
/// issuer-only walks (which skip index 0 because a leaf issues nothing), *every*
/// certificate must be time-valid, so the walk covers the whole list including the leaf.
/// `now` is seconds since the Unix epoch, threaded in from the connect loop exactly as
/// [`verify_certificate_validity`] takes it. It does **not** verify signatures, names,
/// CA permission, or terminate the chain at a trust anchor — those are the sibling walks.
///
/// # Errors
///
/// [`ValidityWalkError::EmptyChain`] if `chain` is empty, or
/// [`ValidityWalkError::Certificate`] naming the first certificate whose DER failed to
/// decode or that was outside its validity window at `now`.
pub fn verify_validity_chain(chain: &[&[u8]], now: i64) -> Result<(), ValidityWalkError> {
    if chain.is_empty() {
        return Err(ValidityWalkError::EmptyChain);
    }
    // Every certificate in the path — leaf and each issuer — must be within its own
    // validity window; the walk stops at the first that is not.
    for (index, cert_der) in chain.iter().enumerate() {
        verify_certificate_validity(cert_der, now)
            .map_err(|error| ValidityWalkError::Certificate { index, error })?;
    }
    Ok(())
}

/// Decode one `Time` value (RFC 5280 §4.1.2.5) — a `UTCTime` (`0x17`) or a
/// `GeneralizedTime` (`0x18`) — into seconds since the Unix epoch.
fn parse_time(tag: u8, bytes: &[u8]) -> Result<i64, ValidityError> {
    // RFC 5280 pins both forms to UTC with mandatory seconds and a 'Z' suffix.
    let (year, rest) = match tag {
        TAG_UTC_TIME => {
            // "YYMMDDHHMMSSZ" — 13 octets.
            if bytes.len() != 13 {
                return Err(ValidityError::MalformedTime("UTCTime is not YYMMDDHHMMSSZ"));
            }
            let yy = two_digits(&bytes[0..2])?;
            // RFC 5280 §4.1.2.5.1: 50..=99 is 19YY, 00..=49 is 20YY.
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            (year as i64, &bytes[2..])
        }
        TAG_GENERALIZED_TIME => {
            // "YYYYMMDDHHMMSSZ" — 15 octets.
            if bytes.len() != 15 {
                return Err(ValidityError::MalformedTime(
                    "GeneralizedTime is not YYYYMMDDHHMMSSZ",
                ));
            }
            let year = two_digits(&bytes[0..2])? * 100 + two_digits(&bytes[2..4])?;
            (year as i64, &bytes[4..])
        }
        _ => return Err(ValidityError::MalformedTime("Time is neither UTCTime nor GeneralizedTime")),
    };

    // `rest` is "MMDDHHMMSSZ" for both forms (11 octets).
    let month = two_digits(&rest[0..2])? as i64;
    let day = two_digits(&rest[2..4])? as i64;
    let hour = two_digits(&rest[4..6])? as i64;
    let minute = two_digits(&rest[6..8])? as i64;
    let second = two_digits(&rest[8..10])? as i64;
    if rest[10] != b'Z' {
        return Err(ValidityError::MalformedTime("Time does not end in 'Z' (UTC)"));
    }

    // Field ranges. Seconds up to 60 tolerate a leap second; day-of-month is bounded
    // per month (with a leap-year February) so an impossible date is rejected.
    if !(1..=12).contains(&month)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return Err(ValidityError::MalformedTime("Time field out of range"));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(ValidityError::MalformedTime("Time day-of-month out of range"));
    }

    let days = days_from_civil(year, month, day);
    Ok(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Parse exactly two ASCII digits into a `u32`, rejecting any non-digit octet.
fn two_digits(pair: &[u8]) -> Result<u32, ValidityError> {
    let hi = pair[0];
    let lo = pair[1];
    if !hi.is_ascii_digit() || !lo.is_ascii_digit() {
        return Err(ValidityError::MalformedTime("Time has a non-digit octet"));
    }
    Ok((hi - b'0') as u32 * 10 + (lo - b'0') as u32)
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days in `month` (1..=12) of `year`, honouring leap-year February.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from the Unix epoch (1970-01-01) to `y-m-d`, by Howard Hinnant's
/// `days_from_civil` algorithm. `m` is 1..=12 and `d` is 1..=31; the result is
/// negative for dates before the epoch. Valid across the whole proleptic Gregorian
/// calendar, which covers every RFC 5280 certificate date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Shift January/February to the end of the previous year so leap day is last.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // year of era, [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // day of era, [0, 146096]
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DER construction helpers (test-only certificate builder) ───────────

    /// Encode a DER definite length: short form under 128, long form otherwise.
    fn encode_len(len: usize, out: &mut Vec<u8>) {
        if len < 0x80 {
            out.push(len as u8);
            return;
        }
        let mut octets = len.to_be_bytes().to_vec();
        while octets.first() == Some(&0) {
            octets.remove(0);
        }
        out.push(0x80 | octets.len() as u8);
        out.extend_from_slice(&octets);
    }

    /// Build a `tag ‖ length ‖ contents` TLV.
    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        encode_len(contents.len(), &mut out);
        out.extend_from_slice(contents);
        out
    }

    /// Concatenate several DER blobs.
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// A `UTCTime` TLV from a `YYMMDDHHMMSSZ` string.
    fn utc_time(s: &str) -> Vec<u8> {
        tlv(TAG_UTC_TIME, s.as_bytes())
    }

    /// A `GeneralizedTime` TLV from a `YYYYMMDDHHMMSSZ` string.
    fn generalized_time(s: &str) -> Vec<u8> {
        tlv(TAG_GENERALIZED_TIME, s.as_bytes())
    }

    /// A `Validity` SEQUENCE around two `Time` TLVs.
    fn validity(not_before: &[u8], not_after: &[u8]) -> Vec<u8> {
        tlv(TAG_SEQUENCE, &cat(&[not_before, not_after]))
    }

    /// Assemble a minimal but structurally valid v3 certificate carrying the given
    /// `validity` field. Every other field is a placeholder the walker skips.
    fn cert_with_validity(validity_field: &[u8]) -> Vec<u8> {
        let version = tlv(TAG_CONTEXT_0, &tlv(TAG_INTEGER, &[0x02])); // [0] { v3 = 2 }
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[&version, &serial, &sig_alg, &issuer, validity_field, &subject, &spki]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]))
    }

    // ── days_from_civil: pinned to known epoch offsets ─────────────────────

    #[test]
    fn days_from_civil_matches_known_anchors() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 1, 1), 10957); // 30 years, 7 leap days
        // A day beyond the leap day of a leap year.
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
    }

    #[test]
    fn parse_time_computes_unix_seconds() {
        // 2000-01-01T00:00:00Z = 946684800 (a well-known epoch anchor).
        assert_eq!(
            parse_time(TAG_UTC_TIME, b"000101000000Z"),
            Ok(946_684_800),
        );
        // 1970-01-01T00:00:01Z = 1.
        assert_eq!(parse_time(TAG_UTC_TIME, b"700101000001Z"), Ok(1));
        // A GeneralizedTime past 2049: 2050-01-01T00:00:00Z = 2524608000.
        assert_eq!(
            parse_time(TAG_GENERALIZED_TIME, b"20500101000000Z"),
            Ok(2_524_608_000),
        );
    }

    // ── the two-digit-year window (RFC 5280 §4.1.2.5.1) ────────────────────

    #[test]
    fn utc_time_year_window_splits_at_fifty() {
        // 49 -> 2049, 50 -> 1950.
        let y2049 = parse_time(TAG_UTC_TIME, b"490101000000Z").expect("parses");
        let y1950 = parse_time(TAG_UTC_TIME, b"500101000000Z").expect("parses");
        assert_eq!(y2049, days_from_civil(2049, 1, 1) * 86_400);
        assert_eq!(y1950, days_from_civil(1950, 1, 1) * 86_400);
        assert!(y1950 < y2049);
    }

    // ── the happy path: now inside the window ──────────────────────────────

    #[test]
    fn accepts_a_certificate_valid_now() {
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        // 2025-01-01T00:00:00Z sits inside [2020, 2030).
        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert_eq!(verify_certificate_validity(&cert, now), Ok(()));
    }

    #[test]
    fn accepts_a_generalized_time_window() {
        let v = validity(
            &generalized_time("20500101000000Z"),
            &generalized_time("20600101000000Z"),
        );
        let cert = cert_with_validity(&v);
        let now = days_from_civil(2055, 6, 15) * 86_400 + 12 * 3_600;
        assert_eq!(verify_certificate_validity(&cert, now), Ok(()));
    }

    #[test]
    fn accepts_now_exactly_on_the_boundaries() {
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        let (not_before, not_after) = certificate_validity(&cert).expect("decodes");
        // The window is inclusive at both ends.
        assert_eq!(verify_certificate_validity(&cert, not_before), Ok(()));
        assert_eq!(verify_certificate_validity(&cert, not_after), Ok(()));
    }

    // ── expiry / not-yet-valid ─────────────────────────────────────────────

    #[test]
    fn rejects_a_not_yet_valid_certificate() {
        let v = validity(&utc_time("300101000000Z"), &utc_time("400101000000Z"));
        let cert = cert_with_validity(&v);
        let now = days_from_civil(2025, 1, 1) * 86_400;
        let not_before = days_from_civil(2030, 1, 1) * 86_400;
        assert_eq!(
            verify_certificate_validity(&cert, now),
            Err(ValidityError::NotYetValid { not_before, now }),
        );
    }

    #[test]
    fn rejects_an_expired_certificate() {
        let v = validity(&utc_time("100101000000Z"), &utc_time("200101000000Z"));
        let cert = cert_with_validity(&v);
        let now = days_from_civil(2025, 1, 1) * 86_400;
        let not_after = days_from_civil(2020, 1, 1) * 86_400;
        assert_eq!(
            verify_certificate_validity(&cert, now),
            Err(ValidityError::Expired { not_after, now }),
        );
    }

    #[test]
    fn one_second_past_not_after_is_expired() {
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        let (_, not_after) = certificate_validity(&cert).expect("decodes");
        assert!(matches!(
            verify_certificate_validity(&cert, not_after + 1),
            Err(ValidityError::Expired { .. }),
        ));
    }

    #[test]
    fn one_second_before_not_before_is_not_yet_valid() {
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        let (not_before, _) = certificate_validity(&cert).expect("decodes");
        assert!(matches!(
            verify_certificate_validity(&cert, not_before - 1),
            Err(ValidityError::NotYetValid { .. }),
        ));
    }

    // ── certificate_validity: raw bounds ───────────────────────────────────

    #[test]
    fn certificate_validity_returns_both_bounds() {
        let v = validity(&utc_time("200615120000Z"), &utc_time("210615120000Z"));
        let cert = cert_with_validity(&v);
        let (not_before, not_after) = certificate_validity(&cert).expect("decodes");
        assert_eq!(not_before, days_from_civil(2020, 6, 15) * 86_400 + 12 * 3_600);
        assert_eq!(not_after, days_from_civil(2021, 6, 15) * 86_400 + 12 * 3_600);
    }

    #[test]
    fn parses_a_v1_certificate_without_the_version_field() {
        // A v1 cert has no [0] version prefix; the walker must still reach validity.
        let version: &[u8] = &[];
        let serial = tlv(TAG_INTEGER, &[0x01]);
        let sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let issuer = tlv(TAG_SEQUENCE, &[]);
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let subject = tlv(TAG_SEQUENCE, &[]);
        let spki = tlv(TAG_SEQUENCE, &[]);
        let tbs = tlv(
            TAG_SEQUENCE,
            &cat(&[version, &serial, &sig_alg, &issuer, &v, &subject, &spki]),
        );
        let outer_sig_alg = tlv(TAG_SEQUENCE, &tlv(0x06, &[0x2A, 0x03]));
        let signature = tlv(0x03, &[0x00, 0xDE, 0xAD]);
        let cert = tlv(TAG_SEQUENCE, &cat(&[&tbs, &outer_sig_alg, &signature]));

        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert_eq!(verify_certificate_validity(&cert, now), Ok(()));
    }

    // ── malformed Time values ──────────────────────────────────────────────

    #[test]
    fn rejects_a_utc_time_of_wrong_length() {
        // Missing the seconds field (the pre-1996 short form RFC 5280 forbids).
        let v = validity(&utc_time("2001010000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert!(matches!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime(_)),
        ));
    }

    #[test]
    fn rejects_a_time_without_the_z_suffix() {
        // A '+0000'-style offset where 'Z' is required.
        let v = validity(&utc_time("200101000000+"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert_eq!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime("Time does not end in 'Z' (UTC)")),
        );
    }

    #[test]
    fn rejects_a_non_digit_octet() {
        let v = validity(&utc_time("20AB01000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert_eq!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime("Time has a non-digit octet")),
        );
    }

    #[test]
    fn rejects_an_out_of_range_month() {
        let v = validity(&utc_time("201301000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert_eq!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime("Time field out of range")),
        );
    }

    #[test]
    fn rejects_an_impossible_day_of_month() {
        // 2021 is not a leap year: 2021-02-29 does not exist.
        let v = validity(&utc_time("210229000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert_eq!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime("Time day-of-month out of range")),
        );
    }

    #[test]
    fn accepts_a_leap_day_in_a_leap_year() {
        // 2020 is a leap year: 2020-02-29 is valid.
        let v = validity(&utc_time("200229000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert_eq!(verify_certificate_validity(&cert, now), Ok(()));
    }

    #[test]
    fn rejects_a_time_with_an_unexpected_tag() {
        // An INTEGER where a Time is required.
        let bad_time = tlv(TAG_INTEGER, &[0x01]);
        let v = validity(&bad_time, &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        assert_eq!(
            certificate_validity(&cert),
            Err(ValidityError::MalformedTime(
                "Time is neither UTCTime nor GeneralizedTime"
            )),
        );
    }

    // ── malformed certificate structure ────────────────────────────────────

    #[test]
    fn rejects_a_non_sequence_top_level() {
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        assert!(matches!(
            certificate_validity(&not_a_cert),
            Err(ValidityError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_truncated_content() {
        let v = validity(&utc_time("200101000000Z"), &utc_time("300101000000Z"));
        let cert = cert_with_validity(&v);
        let truncated = &cert[..cert.len() - 4];
        assert!(matches!(
            certificate_validity(truncated),
            Err(ValidityError::Malformed(_)),
        ));
    }

    #[test]
    fn rejects_an_empty_input() {
        assert!(matches!(
            certificate_validity(&[]),
            Err(ValidityError::Malformed(_)),
        ));
    }

    // ── chain walk (RFC 5280 §6.1.3(a)(2)) ─────────────────────────────────

    #[test]
    fn walk_accepts_a_chain_all_within_validity() {
        let leaf = cert_with_validity(&validity(
            &utc_time("200101000000Z"),
            &utc_time("300101000000Z"),
        ));
        let intermediate = cert_with_validity(&validity(
            &utc_time("150101000000Z"),
            &utc_time("350101000000Z"),
        ));
        let chain = [leaf.as_slice(), intermediate.as_slice()];
        // 2025-01-01 sits inside both windows.
        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert_eq!(verify_validity_chain(&chain, now), Ok(()));
    }

    #[test]
    fn walk_rejects_an_empty_chain() {
        assert_eq!(
            verify_validity_chain(&[], 0),
            Err(ValidityWalkError::EmptyChain),
        );
    }

    #[test]
    fn walk_reports_an_expired_intermediate_by_index() {
        // A valid leaf but an intermediate that expired in 2020.
        let leaf = cert_with_validity(&validity(
            &utc_time("200101000000Z"),
            &utc_time("300101000000Z"),
        ));
        let intermediate = cert_with_validity(&validity(
            &utc_time("100101000000Z"),
            &utc_time("200101000000Z"),
        ));
        let chain = [leaf.as_slice(), intermediate.as_slice()];
        let now = days_from_civil(2025, 1, 1) * 86_400;
        let not_after = days_from_civil(2020, 1, 1) * 86_400;
        assert_eq!(
            verify_validity_chain(&chain, now),
            Err(ValidityWalkError::Certificate {
                index: 1,
                error: ValidityError::Expired { not_after, now },
            }),
        );
    }

    #[test]
    fn walk_covers_the_leaf_at_index_zero() {
        // Unlike the issuer-only walks, the leaf itself must be time-valid.
        let leaf = cert_with_validity(&validity(
            &utc_time("300101000000Z"),
            &utc_time("400101000000Z"),
        ));
        let intermediate = cert_with_validity(&validity(
            &utc_time("100101000000Z"),
            &utc_time("400101000000Z"),
        ));
        let chain = [leaf.as_slice(), intermediate.as_slice()];
        let now = days_from_civil(2025, 1, 1) * 86_400;
        let not_before = days_from_civil(2030, 1, 1) * 86_400;
        assert_eq!(
            verify_validity_chain(&chain, now),
            Err(ValidityWalkError::Certificate {
                index: 0,
                error: ValidityError::NotYetValid { not_before, now },
            }),
        );
    }

    #[test]
    fn walk_reports_a_malformed_certificate_by_index() {
        let leaf = cert_with_validity(&validity(
            &utc_time("200101000000Z"),
            &utc_time("300101000000Z"),
        ));
        let not_a_cert = tlv(TAG_INTEGER, &[0x01]);
        let chain = [leaf.as_slice(), not_a_cert.as_slice()];
        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert!(matches!(
            verify_validity_chain(&chain, now),
            Err(ValidityWalkError::Certificate {
                index: 1,
                error: ValidityError::Malformed(_),
            }),
        ));
    }

    #[test]
    fn walk_accepts_a_single_certificate_list() {
        let leaf = cert_with_validity(&validity(
            &utc_time("200101000000Z"),
            &utc_time("300101000000Z"),
        ));
        let chain = [leaf.as_slice()];
        let now = days_from_civil(2025, 1, 1) * 86_400;
        assert_eq!(verify_validity_chain(&chain, now), Ok(()));
    }
}
