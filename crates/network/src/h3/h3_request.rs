//! HTTP/3 request/response message translation (RFC 9114 §4.1–§4.3).
//!
//! This is the semantic bridge between an HTTP request/response and the QPACK
//! field section carried in a `HEADERS` frame — the request-path counterpart of
//! [`crate::h3::qpack`] (which only turns a field *list* into bytes) and
//! [`crate::h3::frame`] (which only frames an *opaque* field block). Where the
//! HTTP/2 side lives in `h2::conn` (pseudo-header ordering + `fetch`), the
//! HTTP/3 side is factored out as a pure module because the whole QUIC transport
//! below it is still being assembled slice by slice.
//!
//! What it does, all without IO:
//! - builds the ordered request field list — the four request pseudo-headers
//!   (`:method`, `:scheme`, `:authority`, `:path`, RFC 9114 §4.3.1) in the
//!   fingerprint order of the impersonated browser, followed by the regular
//!   fields — and QPACK-encodes it into a `HEADERS` frame ([`encode_request`]);
//! - decodes a response `HEADERS` frame's field block back into the `:status`
//!   code and the ordinary header list ([`decode_response`]), enforcing the
//!   RFC 9114 §4.1.2/§4.2/§4.3.2 well-formedness rules a client must treat a
//!   received response against (exactly one `:status` first, no request/unknown
//!   pseudo-headers, lower-case field names, no connection-specific fields).
//!
//! Out of scope: request bodies (a separate `DATA` frame), trailers, the QPACK
//! *dynamic* table (this uses the static-only [`qpack::encode_field_section`] /
//! [`qpack::decode_field_section`]), and the transport that actually carries the
//! bytes.

use super::frame::{Frame, FrameError};
use super::qpack::{self, HeaderField, QpackError};

/// A list of ordinary header/trailer fields as decoded `(name, value)` byte-string
/// pairs, in received order.
pub type FieldPairs = Vec<(Vec<u8>, Vec<u8>)>;

/// The HTTP/2 (and thus HTTP/3) impersonation profile, selecting the pseudo-
/// header order that forms part of the request fingerprint.
///
/// The request pseudo-header order is observable on the wire and anti-bot
/// layers key on it, so it must match the browser Lumen impersonates. Mirrors
/// the ordering `h2::conn` applies to HPACK.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum H3Profile {
    /// Chrome / Edge order `:method :authority :scheme :path` — Lumen's default.
    #[default]
    Chrome,
    /// Firefox / Tor Browser order `:method :path :authority :scheme`.
    Firefox,
    /// Safari order `:method :scheme :path :authority`.
    Safari,
}

/// An error translating between an HTTP message and its QPACK field section.
///
/// The malformed-message variants correspond to the RFC 9114 §4.1.2 rule that a
/// receiver treats a malformed message as a stream error of type
/// `H3_MESSAGE_ERROR`; the transport reports that code, so this type stays a
/// plain diagnosis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageError {
    /// A response carried no `:status` pseudo-header (RFC 9114 §4.3.2).
    MissingStatus,
    /// The `:status` value was not a three-digit `100`–`599` code.
    InvalidStatus(Vec<u8>),
    /// A pseudo-header (`:`-prefixed) appeared after an ordinary field
    /// (RFC 9114 §4.3): all pseudo-headers must precede the regular fields.
    PseudoAfterRegular(Vec<u8>),
    /// A response field section carried a pseudo-header other than `:status`
    /// (RFC 9114 §4.3.2 — request pseudo-headers or an unknown `:name`).
    UnexpectedPseudo(Vec<u8>),
    /// A field name contained an upper-case octet (RFC 9114 §4.2 requires
    /// lower-case field names; a message with an upper-case name is malformed).
    UppercaseName(Vec<u8>),
    /// A connection-specific field forbidden in HTTP/3 was present
    /// (`connection` / `keep-alive` / `proxy-connection` / `transfer-encoding`
    /// / `upgrade`, or a `te` whose value is not exactly `trailers`;
    /// RFC 9114 §4.2).
    ConnectionSpecificField(Vec<u8>),
    /// A field name was empty.
    EmptyName,
    /// A trailer section carried a pseudo-header field (`:`-prefixed), which
    /// RFC 9114 §4.1 forbids: the trailer section must contain only ordinary
    /// fields.
    PseudoInTrailer(Vec<u8>),
    /// The QPACK field section failed to decode.
    Qpack(QpackError),
    /// The `HEADERS` frame failed to encode/decode.
    Frame(FrameError),
}

impl core::fmt::Display for MessageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingStatus => write!(f, "response has no :status pseudo-header"),
            Self::InvalidStatus(v) => {
                write!(f, "invalid :status value {:?}", String::from_utf8_lossy(v))
            }
            Self::PseudoAfterRegular(n) => write!(
                f,
                "pseudo-header {:?} after a regular field",
                String::from_utf8_lossy(n)
            ),
            Self::UnexpectedPseudo(n) => write!(
                f,
                "unexpected pseudo-header {:?} in response",
                String::from_utf8_lossy(n)
            ),
            Self::UppercaseName(n) => write!(
                f,
                "upper-case field name {:?}",
                String::from_utf8_lossy(n)
            ),
            Self::ConnectionSpecificField(n) => write!(
                f,
                "connection-specific field {:?} forbidden in HTTP/3",
                String::from_utf8_lossy(n)
            ),
            Self::EmptyName => write!(f, "empty field name"),
            Self::PseudoInTrailer(n) => write!(
                f,
                "pseudo-header {:?} in trailer section",
                String::from_utf8_lossy(n)
            ),
            Self::Qpack(e) => write!(f, "QPACK: {e}"),
            Self::Frame(e) => write!(f, "HEADERS frame: {e}"),
        }
    }
}

impl std::error::Error for MessageError {}

impl From<QpackError> for MessageError {
    fn from(e: QpackError) -> Self {
        Self::Qpack(e)
    }
}

impl From<FrameError> for MessageError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// The five connection-specific field names an HTTP/3 message must not carry
/// (RFC 9114 §4.2). `te` is handled separately because it is allowed with the
/// single value `trailers`.
const FORBIDDEN_FIELDS: [&[u8]; 5] = [
    b"connection",
    b"keep-alive",
    b"proxy-connection",
    b"transfer-encoding",
    b"upgrade",
];

/// A decoded HTTP/3 response head: the `:status` code and the ordinary header
/// fields (pseudo-headers stripped), in received order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H3ResponseHead {
    /// The `:status` code (RFC 9114 §4.3.2), a value in `100`–`599`.
    pub status: u16,
    /// The ordinary response header fields, lower-case names, in order.
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Return `true` if `name` is a valid lower-case field name octet string with
/// no upper-case ASCII letters (RFC 9114 §4.2). Non-ASCII octets are left to
/// the QPACK layer; only the upper-case rule is a message-level malformation.
fn has_uppercase(name: &[u8]) -> bool {
    name.iter().any(u8::is_ascii_uppercase)
}

/// Validate an ordinary (non-pseudo) field against RFC 9114 §4.2: lower-case
/// name, not empty, and not a forbidden connection-specific field.
fn validate_regular_field(name: &[u8], value: &[u8]) -> Result<(), MessageError> {
    if name.is_empty() {
        return Err(MessageError::EmptyName);
    }
    if has_uppercase(name) {
        return Err(MessageError::UppercaseName(name.to_vec()));
    }
    if FORBIDDEN_FIELDS.contains(&name) {
        return Err(MessageError::ConnectionSpecificField(name.to_vec()));
    }
    // TE is permitted only with the exact value "trailers" (RFC 9114 §4.2).
    if name == b"te" && value != b"trailers" {
        return Err(MessageError::ConnectionSpecificField(name.to_vec()));
    }
    Ok(())
}

/// Build the ordered request field list: the four request pseudo-headers in the
/// profile's fingerprint order (RFC 9114 §4.3.1), then the regular fields.
///
/// `extra_headers` are ordinary request headers as `(name, value)` byte slices;
/// their names must be lower-case and must not be pseudo-headers or
/// connection-specific fields — each is validated per RFC 9114 §4.2.
///
/// # Errors
/// [`MessageError`] if any `extra_headers` entry violates RFC 9114 §4.2.
pub fn build_request_fields(
    profile: H3Profile,
    method: &[u8],
    scheme: &[u8],
    authority: &[u8],
    path: &[u8],
    extra_headers: &[(&[u8], &[u8])],
) -> Result<Vec<HeaderField>, MessageError> {
    let m = HeaderField::new(b":method".to_vec(), method.to_vec());
    let s = HeaderField::new(b":scheme".to_vec(), scheme.to_vec());
    let a = HeaderField::new(b":authority".to_vec(), authority.to_vec());
    let p = HeaderField::new(b":path".to_vec(), path.to_vec());

    let mut fields = match profile {
        H3Profile::Chrome => vec![m, a, s, p],
        H3Profile::Firefox => vec![m, p, a, s],
        H3Profile::Safari => vec![m, s, p, a],
    };

    for (name, value) in extra_headers {
        validate_regular_field(name, value)?;
        fields.push(HeaderField::new(name.to_vec(), value.to_vec()));
    }
    Ok(fields)
}

/// Encode a request into a complete `HEADERS` frame (RFC 9114 §4.1, §7.2.2):
/// build the profile-ordered field list, QPACK-encode it (static table only),
/// and wrap it in a `HEADERS` frame ready to write to the request stream.
///
/// `use_huffman` enables Huffman coding of literal names/values when it does not
/// enlarge them.
///
/// # Errors
/// [`MessageError`] if a request header violates RFC 9114 §4.2 or the frame
/// fails to encode.
pub fn encode_request(
    profile: H3Profile,
    method: &[u8],
    scheme: &[u8],
    authority: &[u8],
    path: &[u8],
    extra_headers: &[(&[u8], &[u8])],
    use_huffman: bool,
) -> Result<Vec<u8>, MessageError> {
    let fields = build_request_fields(profile, method, scheme, authority, path, extra_headers)?;
    let block = qpack::encode_field_section(&fields, use_huffman);
    let mut out = Vec::new();
    Frame::Headers(block).encode(&mut out)?;
    Ok(out)
}

/// Parse a `:status` value: exactly three ASCII digits forming a `100`–`599`
/// code (RFC 9114 §4.3.2, RFC 9110 §15).
fn parse_status(value: &[u8]) -> Result<u16, MessageError> {
    if value.len() != 3 || !value.iter().all(u8::is_ascii_digit) {
        return Err(MessageError::InvalidStatus(value.to_vec()));
    }
    // Three ASCII digits: safe to parse.
    let code = (u16::from(value[0] - b'0')) * 100
        + (u16::from(value[1] - b'0')) * 10
        + u16::from(value[2] - b'0');
    if (100..=599).contains(&code) {
        Ok(code)
    } else {
        Err(MessageError::InvalidStatus(value.to_vec()))
    }
}

/// Validate a decoded response field list (RFC 9114 §4.1.2, §4.2, §4.3.2) and
/// split it into `:status` and the ordinary header fields.
///
/// Rules enforced: exactly one `:status` (the only pseudo-header allowed in a
/// response) and it precedes every ordinary field; no ordinary field follows
/// by a pseudo-header; lower-case names; no connection-specific fields.
///
/// # Errors
/// [`MessageError`] on any malformed-message condition above.
pub fn validate_response_fields(fields: &[HeaderField]) -> Result<H3ResponseHead, MessageError> {
    let mut status: Option<u16> = None;
    let mut headers: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut seen_regular = false;

    for field in fields {
        let name = field.name.as_slice();
        if name.first() == Some(&b':') {
            // Pseudo-header: must precede all regular fields (RFC 9114 §4.3).
            if seen_regular {
                return Err(MessageError::PseudoAfterRegular(name.to_vec()));
            }
            if name == b":status" {
                // Exactly one :status (a duplicate is malformed, §4.3.2).
                if status.is_some() {
                    return Err(MessageError::UnexpectedPseudo(name.to_vec()));
                }
                status = Some(parse_status(&field.value)?);
            } else {
                // Any other pseudo-header is invalid in a response (§4.3.2).
                return Err(MessageError::UnexpectedPseudo(name.to_vec()));
            }
        } else {
            seen_regular = true;
            validate_regular_field(name, &field.value)?;
            headers.push((field.name.clone(), field.value.clone()));
        }
    }

    let status = status.ok_or(MessageError::MissingStatus)?;
    Ok(H3ResponseHead { status, headers })
}

/// Decode a response `HEADERS` frame's QPACK field block into an
/// [`H3ResponseHead`] (RFC 9114 §4.1, §4.3.2).
///
/// `block` is the field section carried by the `HEADERS` frame — the payload of
/// [`Frame::Headers`], i.e. the caller has already stripped the frame header.
///
/// # Errors
/// [`MessageError`] if the QPACK block fails to decode or the response is
/// malformed (RFC 9114 §4.1.2).
pub fn decode_response(block: &[u8]) -> Result<H3ResponseHead, MessageError> {
    let fields = qpack::decode_field_section(block)?;
    validate_response_fields(&fields)
}

/// Validate a decoded trailer field list (RFC 9114 §4.1) and return the ordinary
/// header fields.
///
/// A trailer section is a header section that MUST NOT contain any pseudo-header
/// field (RFC 9114 §4.1); every field is otherwise held to the same RFC 9114 §4.2
/// rules as a regular header (lower-case, non-empty, no connection-specific
/// field).
///
/// # Errors
/// [`MessageError::PseudoInTrailer`] if a `:`-prefixed field is present, or any
/// other [`MessageError`] variant for an RFC 9114 §4.2 violation.
pub fn validate_trailer_fields(fields: &[HeaderField]) -> Result<FieldPairs, MessageError> {
    let mut headers = Vec::with_capacity(fields.len());
    for field in fields {
        let name = field.name.as_slice();
        if name.first() == Some(&b':') {
            return Err(MessageError::PseudoInTrailer(name.to_vec()));
        }
        validate_regular_field(name, &field.value)?;
        headers.push((field.name.clone(), field.value.clone()));
    }
    Ok(headers)
}

/// Decode a trailer `HEADERS` frame's QPACK field block into the ordinary trailer
/// fields (RFC 9114 §4.1).
///
/// `block` is the field section carried by the trailing `HEADERS` frame — the
/// payload of [`Frame::Headers`], i.e. the caller has already stripped the frame
/// header.
///
/// # Errors
/// [`MessageError`] if the QPACK block fails to decode or the trailer section is
/// malformed (a pseudo-header, an upper-case or empty name, or a
/// connection-specific field).
pub fn decode_trailers(block: &[u8]) -> Result<FieldPairs, MessageError> {
    let fields = qpack::decode_field_section(block)?;
    validate_trailer_fields(&fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::frame::Frame;

    /// Extract a field by name from a list, returning its value.
    fn field<'a>(fields: &'a [HeaderField], name: &[u8]) -> Option<&'a [u8]> {
        fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.as_slice())
    }

    #[test]
    fn chrome_pseudo_header_order() {
        let fields =
            build_request_fields(H3Profile::Chrome, b"GET", b"https", b"example.com", b"/", &[])
                .unwrap();
        let names: Vec<&[u8]> = fields.iter().map(|f| f.name.as_slice()).collect();
        assert_eq!(
            names,
            vec![
                b":method".as_slice(),
                b":authority",
                b":scheme",
                b":path"
            ]
        );
    }

    #[test]
    fn firefox_and_safari_pseudo_header_order() {
        let ff =
            build_request_fields(H3Profile::Firefox, b"GET", b"https", b"h", b"/", &[]).unwrap();
        let ff_names: Vec<&[u8]> = ff.iter().map(|f| f.name.as_slice()).collect();
        assert_eq!(
            ff_names,
            vec![b":method".as_slice(), b":path", b":authority", b":scheme"]
        );

        let sf =
            build_request_fields(H3Profile::Safari, b"GET", b"https", b"h", b"/", &[]).unwrap();
        let sf_names: Vec<&[u8]> = sf.iter().map(|f| f.name.as_slice()).collect();
        assert_eq!(
            sf_names,
            vec![b":method".as_slice(), b":scheme", b":path", b":authority"]
        );
    }

    #[test]
    fn request_pseudo_values_and_extra_headers() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"POST",
            b"https",
            b"example.com",
            b"/submit",
            &[(b"accept", b"text/html"), (b"user-agent", b"Lumen")],
        )
        .unwrap();
        assert_eq!(field(&fields, b":method"), Some(b"POST".as_slice()));
        assert_eq!(field(&fields, b":scheme"), Some(b"https".as_slice()));
        assert_eq!(field(&fields, b":authority"), Some(b"example.com".as_slice()));
        assert_eq!(field(&fields, b":path"), Some(b"/submit".as_slice()));
        assert_eq!(field(&fields, b"accept"), Some(b"text/html".as_slice()));
        // Regular fields follow the four pseudo-headers.
        assert_eq!(fields[4].name, b"accept");
    }

    #[test]
    fn reject_uppercase_request_header() {
        let err = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"h",
            b"/",
            &[(b"Accept", b"x")],
        )
        .unwrap_err();
        assert_eq!(err, MessageError::UppercaseName(b"Accept".to_vec()));
    }

    #[test]
    fn reject_connection_specific_request_header() {
        for bad in [
            b"connection".as_slice(),
            b"keep-alive",
            b"proxy-connection",
            b"transfer-encoding",
            b"upgrade",
        ] {
            let err = build_request_fields(
                H3Profile::Chrome,
                b"GET",
                b"https",
                b"h",
                b"/",
                &[(bad, b"x")],
            )
            .unwrap_err();
            assert_eq!(err, MessageError::ConnectionSpecificField(bad.to_vec()));
        }
    }

    #[test]
    fn te_trailers_allowed_other_te_rejected() {
        // te: trailers is the one permitted TE value (RFC 9114 §4.2).
        assert!(
            build_request_fields(
                H3Profile::Chrome,
                b"GET",
                b"https",
                b"h",
                b"/",
                &[(b"te", b"trailers")]
            )
            .is_ok()
        );
        let err = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"h",
            b"/",
            &[(b"te", b"gzip")],
        )
        .unwrap_err();
        assert_eq!(err, MessageError::ConnectionSpecificField(b"te".to_vec()));
    }

    #[test]
    fn encode_request_produces_headers_frame() {
        let frame_bytes =
            encode_request(H3Profile::Chrome, b"GET", b"https", b"example.com", b"/", &[], true)
                .unwrap();
        // The bytes parse back as a HEADERS frame whose block decodes to the
        // request field list.
        let (frame, consumed) = Frame::parse(&frame_bytes).unwrap().unwrap();
        assert_eq!(consumed, frame_bytes.len());
        let Frame::Headers(block) = frame else {
            panic!("expected HEADERS frame");
        };
        let fields = qpack::decode_field_section(&block).unwrap();
        assert_eq!(field(&fields, b":method"), Some(b"GET".as_slice()));
        assert_eq!(field(&fields, b":path"), Some(b"/".as_slice()));
    }

    #[test]
    fn decode_valid_response() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"content-type".to_vec(), b"text/html".to_vec()),
            HeaderField::new(b"content-length".to_vec(), b"1234".to_vec()),
        ];
        let block = qpack::encode_field_section(&fields, true);
        let head = decode_response(&block).unwrap();
        assert_eq!(head.status, 200);
        assert_eq!(
            head.headers,
            vec![
                (b"content-type".to_vec(), b"text/html".to_vec()),
                (b"content-length".to_vec(), b"1234".to_vec()),
            ]
        );
    }

    #[test]
    fn response_missing_status_is_malformed() {
        let fields = vec![HeaderField::new(b"content-type".to_vec(), b"text/html".to_vec())];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::MissingStatus
        );
    }

    #[test]
    fn response_request_pseudo_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b":path".to_vec(), b"/".to_vec()),
        ];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::UnexpectedPseudo(b":path".to_vec())
        );
    }

    #[test]
    fn response_duplicate_status_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b":status".to_vec(), b"204".to_vec()),
        ];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::UnexpectedPseudo(b":status".to_vec())
        );
    }

    #[test]
    fn response_pseudo_after_regular_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"content-type".to_vec(), b"text/html".to_vec()),
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
        ];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::PseudoAfterRegular(b":status".to_vec())
        );
    }

    #[test]
    fn response_uppercase_field_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"Content-Type".to_vec(), b"text/html".to_vec()),
        ];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::UppercaseName(b"Content-Type".to_vec())
        );
    }

    #[test]
    fn response_connection_specific_field_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"transfer-encoding".to_vec(), b"chunked".to_vec()),
        ];
        assert_eq!(
            validate_response_fields(&fields).unwrap_err(),
            MessageError::ConnectionSpecificField(b"transfer-encoding".to_vec())
        );
    }

    #[test]
    fn invalid_status_values_rejected() {
        for bad in [b"20".as_slice(), b"2000", b"20x", b"099", b"600", b"abc"] {
            let fields = vec![HeaderField::new(b":status".to_vec(), bad.to_vec())];
            assert_eq!(
                validate_response_fields(&fields).unwrap_err(),
                MessageError::InvalidStatus(bad.to_vec())
            );
        }
    }

    #[test]
    fn status_boundary_values_accepted() {
        for good in [b"100".as_slice(), b"200", b"404", b"500", b"599"] {
            let fields = vec![HeaderField::new(b":status".to_vec(), good.to_vec())];
            let head = validate_response_fields(&fields).unwrap();
            let expected: u16 = std::str::from_utf8(good).unwrap().parse().unwrap();
            assert_eq!(head.status, expected);
        }
    }

    #[test]
    fn request_response_round_trip_via_frames() {
        // Encode a request, then encode a matching response, decode both.
        let req_frame =
            encode_request(H3Profile::Chrome, b"GET", b"https", b"h", b"/index.html", &[], true)
                .unwrap();
        let (Frame::Headers(req_block), _) = Frame::parse(&req_frame).unwrap().unwrap() else {
            panic!("expected HEADERS");
        };
        let req_fields = qpack::decode_field_section(&req_block).unwrap();
        assert_eq!(field(&req_fields, b":path"), Some(b"/index.html".as_slice()));

        let resp_fields = vec![
            HeaderField::new(b":status".to_vec(), b"404".to_vec()),
            HeaderField::new(b"content-type".to_vec(), b"text/plain".to_vec()),
        ];
        let resp_block = qpack::encode_field_section(&resp_fields, true);
        let mut resp_frame = Vec::new();
        Frame::Headers(resp_block).encode(&mut resp_frame).unwrap();
        let (Frame::Headers(block), _) = Frame::parse(&resp_frame).unwrap().unwrap() else {
            panic!("expected HEADERS");
        };
        let head = decode_response(&block).unwrap();
        assert_eq!(head.status, 404);
        assert_eq!(head.headers.len(), 1);
    }

    #[test]
    fn decode_valid_trailers() {
        let fields = vec![
            HeaderField::new(b"x-checksum".to_vec(), b"abc123".to_vec()),
            HeaderField::new(b"x-signature".to_vec(), b"deadbeef".to_vec()),
        ];
        let block = qpack::encode_field_section(&fields, true);
        let trailers = decode_trailers(&block).unwrap();
        assert_eq!(
            trailers,
            vec![
                (b"x-checksum".to_vec(), b"abc123".to_vec()),
                (b"x-signature".to_vec(), b"deadbeef".to_vec()),
            ]
        );
    }

    #[test]
    fn trailer_pseudo_header_is_malformed() {
        let fields = vec![
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"x-checksum".to_vec(), b"abc".to_vec()),
        ];
        assert_eq!(
            validate_trailer_fields(&fields).unwrap_err(),
            MessageError::PseudoInTrailer(b":status".to_vec())
        );
    }

    #[test]
    fn trailer_connection_specific_field_is_malformed() {
        let fields = vec![HeaderField::new(
            b"transfer-encoding".to_vec(),
            b"chunked".to_vec(),
        )];
        assert_eq!(
            validate_trailer_fields(&fields).unwrap_err(),
            MessageError::ConnectionSpecificField(b"transfer-encoding".to_vec())
        );
    }

    #[test]
    fn empty_trailer_section_is_valid() {
        assert!(validate_trailer_fields(&[]).unwrap().is_empty());
    }

    // ── Range / Authorization header fields (срез 101) ────────────────────────

    /// `range` is a regular lowercase header field — RFC 9114 §4.2 allows any
    /// field that passes `validate_regular_field`.
    #[test]
    fn range_header_accepted_as_regular_field() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"example.com",
            b"/file",
            &[(b"range", b"bytes=0-1023")],
        )
        .unwrap();
        assert_eq!(field(&fields, b"range"), Some(b"bytes=0-1023".as_slice()));
    }

    /// `if-range` is a regular lowercase header field (RFC 7233 §3.2).
    #[test]
    fn if_range_header_accepted_as_regular_field() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"example.com",
            b"/file",
            &[
                (b"range", b"bytes=0-499"),
                (b"if-range", b"\"abc123\""),
            ],
        )
        .unwrap();
        assert_eq!(field(&fields, b"if-range"), Some(b"\"abc123\"".as_slice()));
    }

    /// `authorization` is a regular lowercase header field (RFC 7235 §4.2).
    #[test]
    fn authorization_header_accepted_as_regular_field() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"example.com",
            b"/secure",
            &[(b"authorization", b"Bearer token123")],
        )
        .unwrap();
        assert_eq!(
            field(&fields, b"authorization"),
            Some(b"Bearer token123".as_slice()),
        );
    }

    /// Range + If-Range + Authorization together pass validation (all are
    /// standard lowercase regular fields per RFC 9114 §4.2).
    #[test]
    fn range_if_range_authorization_combination_accepted() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"example.com",
            b"/resource",
            &[
                (b"range", b"bytes=100-199"),
                (b"if-range", b"Tue, 01 Jan 2026 00:00:00 GMT"),
                (b"authorization", b"Basic dXNlcjpwYXNz"),
            ],
        )
        .unwrap();
        assert_eq!(field(&fields, b"range"), Some(b"bytes=100-199".as_slice()));
        assert_eq!(
            field(&fields, b"if-range"),
            Some(b"Tue, 01 Jan 2026 00:00:00 GMT".as_slice()),
        );
        assert_eq!(
            field(&fields, b"authorization"),
            Some(b"Basic dXNlcjpwYXNz".as_slice()),
        );
    }

    /// Suffix-range `bytes=-500` encodes and passes through.
    #[test]
    fn suffix_range_header_accepted() {
        let fields = build_request_fields(
            H3Profile::Chrome,
            b"GET",
            b"https",
            b"example.com",
            b"/tail",
            &[(b"range", b"bytes=-500")],
        )
        .unwrap();
        assert_eq!(field(&fields, b"range"), Some(b"bytes=-500".as_slice()));
    }
}
