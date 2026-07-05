//! QUIC transport parameters codec — RFC 9000 §18, §7.4.
//!
//! During the TLS 1.3 handshake each endpoint sends its QUIC transport
//! parameters inside the `quic_transport_parameters` extension
//! ([`EXT_QUIC_TRANSPORT_PARAMETERS`](super::tls_message::EXT_QUIC_TRANSPORT_PARAMETERS),
//! RFC 9001 §8.2). The extension body is a bare sequence of
//!
//! ```text
//! Transport Parameter {
//!   Transport Parameter ID (i),
//!   Transport Parameter Length (i),
//!   Transport Parameter Value (..),
//! }
//! ```
//!
//! entries (RFC 9000 §18), each a QUIC varint id, a varint byte length, and
//! that many value bytes — so this module sits directly on the [`varint`] codec
//! (Slice 1) like every other transport codec here. It is a pure parse/
//! serialize layer: no IO, no TLS, no connection state.
//!
//! The parsed [`TransportParameters`] are what configure the connection state
//! machines built in earlier slices — the peer's `initial_max_data` seeds
//! [`conn_flow::SendConnFlow`](super::conn_flow::SendConnFlow), its
//! `initial_max_stream_data_*` bound [`stream::SendStream`](super::stream::SendStream),
//! its `initial_max_streams_*` seed the [`conn_flow`](super::conn_flow) stream
//! limits, its `max_udp_payload_size` clamps the congestion controller's
//! datagram size, and its `ack_delay_exponent` / `max_ack_delay` scale the
//! ACK-delay handling in [`loss`](super::loss) / [`pto`](super::pto). Wiring
//! those values into a live connection is a later slice; this slice is the codec
//! and the RFC 9000 §18.2 validation.
//!
//! ## Validation (RFC 9000 §7.4, §18.2)
//!
//! - A given parameter id MUST NOT appear more than once
//!   ([`TransportParameterError::DuplicateParameter`]).
//! - Integer parameters carry a single varint whose encoding MUST fill the
//!   parameter Length exactly ([`TransportParameterError::MalformedValue`]).
//! - `max_udp_payload_size` below [`MIN_MAX_UDP_PAYLOAD_SIZE`] (1200),
//!   `ack_delay_exponent` above [`MAX_ACK_DELAY_EXPONENT`] (20),
//!   `max_ack_delay` of [`MAX_ACK_DELAY_LIMIT_MS`] (2^14) or greater, and
//!   `active_connection_id_limit` below [`MIN_ACTIVE_CONNECTION_ID_LIMIT`] (2)
//!   are all [`TransportParameterError::InvalidValue`].
//! - Fixed-width values (`stateless_reset_token`, `disable_active_migration`,
//!   the connection-id length inside a preferred address) are length-checked.
//!
//! Unknown and reserved (GREASE, RFC 9000 §18.1) parameter ids are preserved
//! verbatim in [`TransportParameters::unknown`] and ignored semantically, so a
//! round-trip is byte-stable and forward-compatible.

use super::varint;

// ── Parameter identifiers (RFC 9000 §18.2 registry) ─────────────────────────

/// `original_destination_connection_id` (0x00) — the Destination Connection ID
/// from the client's first Initial packet; server-only (RFC 9000 §18.2).
pub const PARAM_ORIGINAL_DESTINATION_CONNECTION_ID: u64 = 0x00;
/// `max_idle_timeout` (0x01) — idle timeout in milliseconds; `0` disables it.
pub const PARAM_MAX_IDLE_TIMEOUT: u64 = 0x01;
/// `stateless_reset_token` (0x02) — 16-byte token; server-only (RFC 9000 §18.2).
pub const PARAM_STATELESS_RESET_TOKEN: u64 = 0x02;
/// `max_udp_payload_size` (0x03) — largest UDP payload the endpoint will
/// process; default [`DEFAULT_MAX_UDP_PAYLOAD_SIZE`], minimum
/// [`MIN_MAX_UDP_PAYLOAD_SIZE`].
pub const PARAM_MAX_UDP_PAYLOAD_SIZE: u64 = 0x03;
/// `initial_max_data` (0x04) — connection-level flow-control limit.
pub const PARAM_INITIAL_MAX_DATA: u64 = 0x04;
/// `initial_max_stream_data_bidi_local` (0x05) — flow-control limit for
/// bidirectional streams the endpoint itself opens.
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u64 = 0x05;
/// `initial_max_stream_data_bidi_remote` (0x06) — flow-control limit for
/// bidirectional streams the peer opens.
pub const PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u64 = 0x06;
/// `initial_max_stream_data_uni` (0x07) — flow-control limit for
/// unidirectional streams the peer opens.
pub const PARAM_INITIAL_MAX_STREAM_DATA_UNI: u64 = 0x07;
/// `initial_max_streams_bidi` (0x08) — maximum bidirectional streams the peer
/// may open.
pub const PARAM_INITIAL_MAX_STREAMS_BIDI: u64 = 0x08;
/// `initial_max_streams_uni` (0x09) — maximum unidirectional streams the peer
/// may open.
pub const PARAM_INITIAL_MAX_STREAMS_UNI: u64 = 0x09;
/// `ack_delay_exponent` (0x0a) — exponent scaling ACK Delay fields; default
/// [`DEFAULT_ACK_DELAY_EXPONENT`], maximum [`MAX_ACK_DELAY_EXPONENT`].
pub const PARAM_ACK_DELAY_EXPONENT: u64 = 0x0a;
/// `max_ack_delay` (0x0b) — maximum ACK delay in milliseconds; default
/// [`DEFAULT_MAX_ACK_DELAY_MS`], must be below [`MAX_ACK_DELAY_LIMIT_MS`].
pub const PARAM_MAX_ACK_DELAY: u64 = 0x0b;
/// `disable_active_migration` (0x0c) — zero-length flag disabling migration.
pub const PARAM_DISABLE_ACTIVE_MIGRATION: u64 = 0x0c;
/// `preferred_address` (0x0d) — server address the client may migrate to;
/// server-only (RFC 9000 §18.2).
pub const PARAM_PREFERRED_ADDRESS: u64 = 0x0d;
/// `active_connection_id_limit` (0x0e) — connection IDs the endpoint will
/// store; default and minimum [`MIN_ACTIVE_CONNECTION_ID_LIMIT`].
pub const PARAM_ACTIVE_CONNECTION_ID_LIMIT: u64 = 0x0e;
/// `initial_source_connection_id` (0x0f) — the Source Connection ID the
/// endpoint used on its first packet.
pub const PARAM_INITIAL_SOURCE_CONNECTION_ID: u64 = 0x0f;
/// `retry_source_connection_id` (0x10) — the Source Connection ID from a Retry
/// packet; server-only (RFC 9000 §18.2).
pub const PARAM_RETRY_SOURCE_CONNECTION_ID: u64 = 0x10;

// ── Defaults and limits (RFC 9000 §18.2) ────────────────────────────────────

/// Default `max_udp_payload_size` when the parameter is absent (RFC 9000 §18.2).
pub const DEFAULT_MAX_UDP_PAYLOAD_SIZE: u64 = 65527;
/// Smallest legal `max_udp_payload_size`; values below this are invalid
/// (RFC 9000 §18.2).
pub const MIN_MAX_UDP_PAYLOAD_SIZE: u64 = 1200;
/// Default `ack_delay_exponent` when the parameter is absent (RFC 9000 §18.2).
pub const DEFAULT_ACK_DELAY_EXPONENT: u64 = 3;
/// Largest legal `ack_delay_exponent`; values above this are invalid
/// (RFC 9000 §18.2).
pub const MAX_ACK_DELAY_EXPONENT: u64 = 20;
/// Default `max_ack_delay` in milliseconds when the parameter is absent
/// (RFC 9000 §18.2).
pub const DEFAULT_MAX_ACK_DELAY_MS: u64 = 25;
/// Exclusive upper bound for `max_ack_delay` (2^14); a value at or above this
/// is invalid (RFC 9000 §18.2).
pub const MAX_ACK_DELAY_LIMIT_MS: u64 = 1 << 14;
/// Default and minimum `active_connection_id_limit` (RFC 9000 §18.2).
pub const MIN_ACTIVE_CONNECTION_ID_LIMIT: u64 = 2;
/// Length in bytes of a stateless reset token (RFC 9000 §10.3).
pub const STATELESS_RESET_TOKEN_LEN: usize = 16;
/// Largest permitted connection-id length (RFC 9000 §17.2, §5.1.1).
pub const MAX_CONNECTION_ID_LEN: usize = 20;

/// TRANSPORT_PARAMETER_ERROR (RFC 9000 §20.1) — the single wire error code
/// every failure in this codec maps to.
pub const TRANSPORT_PARAMETER_ERROR: u64 = 0x08;

// ── Error ────────────────────────────────────────────────────────────────────

/// Transport-parameters codec error. Every variant is a
/// `TRANSPORT_PARAMETER_ERROR` at the QUIC transport layer (RFC 9000 §7.4);
/// [`TransportParameterError::code`] returns that single wire code and the
/// variant preserves *why* for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportParameterError {
    /// Input ended in the middle of a parameter id, length, or value.
    UnexpectedEof,
    /// A parameter Length claimed more bytes than the buffer holds.
    LengthTooLong,
    /// A varint value exceeds the 2^62 − 1 QUIC varint maximum on encode.
    VarIntOverflow(u64),
    /// The same parameter id appeared more than once (RFC 9000 §7.4). Carries
    /// the offending id.
    DuplicateParameter(u64),
    /// A parameter value did not match the wire shape its id requires (an
    /// integer value that did not fill its Length, a mis-sized fixed-width
    /// value, a truncated preferred address). Carries the parameter id.
    MalformedValue(u64),
    /// A parameter value was well-formed but outside the range RFC 9000 §18.2
    /// permits for its id. Carries the parameter id.
    InvalidValue(u64),
}

impl TransportParameterError {
    /// The RFC 9000 §20.1 wire error code (always `TRANSPORT_PARAMETER_ERROR`).
    #[must_use]
    pub const fn code(&self) -> u64 {
        TRANSPORT_PARAMETER_ERROR
    }
}

impl core::fmt::Display for TransportParameterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "transport parameters: unexpected EOF"),
            Self::LengthTooLong => {
                write!(f, "transport parameters: length exceeds remaining input")
            }
            Self::VarIntOverflow(v) => {
                write!(f, "transport parameters: value {v} exceeds varint maximum")
            }
            Self::DuplicateParameter(id) => {
                write!(f, "transport parameters: duplicate parameter 0x{id:02x}")
            }
            Self::MalformedValue(id) => {
                write!(f, "transport parameters: malformed value for parameter 0x{id:02x}")
            }
            Self::InvalidValue(id) => {
                write!(f, "transport parameters: out-of-range value for parameter 0x{id:02x}")
            }
        }
    }
}

impl std::error::Error for TransportParameterError {}

impl From<varint::VarIntTooLarge> for TransportParameterError {
    fn from(e: varint::VarIntTooLarge) -> Self {
        Self::VarIntOverflow(e.0)
    }
}

// ── Preferred address (RFC 9000 §18.2) ──────────────────────────────────────

/// The `preferred_address` parameter's value: a server address (both an IPv4
/// and an IPv6 endpoint), a new connection ID, and its stateless reset token,
/// which a client may migrate to after the handshake (RFC 9000 §18.2, §9.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferredAddress {
    /// IPv4 address (four octets); all-zero if the server offers no IPv4 address.
    pub ipv4: [u8; 4],
    /// IPv4 port.
    pub ipv4_port: u16,
    /// IPv6 address (sixteen octets); all-zero if the server offers no IPv6
    /// address.
    pub ipv6: [u8; 16],
    /// IPv6 port.
    pub ipv6_port: u16,
    /// The connection ID to use at the preferred address (`0..=20` bytes).
    pub connection_id: Vec<u8>,
    /// The stateless reset token for that connection ID.
    pub stateless_reset_token: [u8; STATELESS_RESET_TOKEN_LEN],
}

impl PreferredAddress {
    /// Parse a preferred address from exactly the parameter value bytes.
    fn parse(value: &[u8]) -> Result<Self, TransportParameterError> {
        let mut buf = value;
        let ipv4 = take_array::<4>(&mut buf, PARAM_PREFERRED_ADDRESS)?;
        let ipv4_port = u16::from_be_bytes(take_array::<2>(&mut buf, PARAM_PREFERRED_ADDRESS)?);
        let ipv6 = take_array::<16>(&mut buf, PARAM_PREFERRED_ADDRESS)?;
        let ipv6_port = u16::from_be_bytes(take_array::<2>(&mut buf, PARAM_PREFERRED_ADDRESS)?);
        let cid_len = take_u8(&mut buf, PARAM_PREFERRED_ADDRESS)? as usize;
        if cid_len > MAX_CONNECTION_ID_LEN {
            return Err(TransportParameterError::MalformedValue(PARAM_PREFERRED_ADDRESS));
        }
        let connection_id = take_bytes(&mut buf, cid_len, PARAM_PREFERRED_ADDRESS)?.to_vec();
        let stateless_reset_token =
            take_array::<STATELESS_RESET_TOKEN_LEN>(&mut buf, PARAM_PREFERRED_ADDRESS)?;
        if !buf.is_empty() {
            // Trailing bytes inside the value that are not part of the address.
            return Err(TransportParameterError::MalformedValue(PARAM_PREFERRED_ADDRESS));
        }
        Ok(Self { ipv4, ipv4_port, ipv6, ipv6_port, connection_id, stateless_reset_token })
    }

    /// Serialize a preferred address into its parameter value bytes.
    fn serialize(&self) -> Result<Vec<u8>, TransportParameterError> {
        if self.connection_id.len() > MAX_CONNECTION_ID_LEN {
            return Err(TransportParameterError::MalformedValue(PARAM_PREFERRED_ADDRESS));
        }
        let mut out = Vec::with_capacity(4 + 2 + 16 + 2 + 1 + self.connection_id.len() + 16);
        out.extend_from_slice(&self.ipv4);
        out.extend_from_slice(&self.ipv4_port.to_be_bytes());
        out.extend_from_slice(&self.ipv6);
        out.extend_from_slice(&self.ipv6_port.to_be_bytes());
        out.push(self.connection_id.len() as u8);
        out.extend_from_slice(&self.connection_id);
        out.extend_from_slice(&self.stateless_reset_token);
        Ok(out)
    }
}

// ── Transport parameters ─────────────────────────────────────────────────────

/// A parsed set of QUIC transport parameters (RFC 9000 §18.2). Every parameter
/// is optional on the wire; a `None` field means the parameter was absent, in
/// which case the RFC 9000 §18.2 default applies (exposed by the accessor
/// methods for the parameters that have one). Unknown / reserved ids are kept
/// verbatim in [`unknown`](Self::unknown).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransportParameters {
    /// `original_destination_connection_id` (server-only).
    pub original_destination_connection_id: Option<Vec<u8>>,
    /// `max_idle_timeout` in milliseconds (`0` / absent = no idle timeout).
    pub max_idle_timeout_ms: Option<u64>,
    /// `stateless_reset_token` (server-only, 16 bytes).
    pub stateless_reset_token: Option<[u8; STATELESS_RESET_TOKEN_LEN]>,
    /// `max_udp_payload_size`; see [`max_udp_payload_size`](Self::max_udp_payload_size).
    pub max_udp_payload_size: Option<u64>,
    /// `initial_max_data` — connection-level flow-control limit (absent = 0).
    pub initial_max_data: Option<u64>,
    /// `initial_max_stream_data_bidi_local` (absent = 0).
    pub initial_max_stream_data_bidi_local: Option<u64>,
    /// `initial_max_stream_data_bidi_remote` (absent = 0).
    pub initial_max_stream_data_bidi_remote: Option<u64>,
    /// `initial_max_stream_data_uni` (absent = 0).
    pub initial_max_stream_data_uni: Option<u64>,
    /// `initial_max_streams_bidi` (absent = 0).
    pub initial_max_streams_bidi: Option<u64>,
    /// `initial_max_streams_uni` (absent = 0).
    pub initial_max_streams_uni: Option<u64>,
    /// `ack_delay_exponent`; see [`ack_delay_exponent`](Self::ack_delay_exponent).
    pub ack_delay_exponent: Option<u64>,
    /// `max_ack_delay` in ms; see [`max_ack_delay_ms`](Self::max_ack_delay_ms).
    pub max_ack_delay_ms: Option<u64>,
    /// `disable_active_migration` — present (zero-length) sets this true.
    pub disable_active_migration: bool,
    /// `preferred_address` (server-only).
    pub preferred_address: Option<PreferredAddress>,
    /// `active_connection_id_limit`; see
    /// [`active_connection_id_limit`](Self::active_connection_id_limit).
    pub active_connection_id_limit: Option<u64>,
    /// `initial_source_connection_id`.
    pub initial_source_connection_id: Option<Vec<u8>>,
    /// `retry_source_connection_id` (server-only).
    pub retry_source_connection_id: Option<Vec<u8>>,
    /// Unknown / reserved (GREASE) parameters, `(id, value)`, preserved in the
    /// order received so a round-trip is byte-stable (RFC 9000 §18.1).
    pub unknown: Vec<(u64, Vec<u8>)>,
}

impl TransportParameters {
    /// Effective `max_udp_payload_size`, applying the RFC 9000 §18.2 default of
    /// [`DEFAULT_MAX_UDP_PAYLOAD_SIZE`] when the parameter is absent.
    #[must_use]
    pub fn max_udp_payload_size(&self) -> u64 {
        self.max_udp_payload_size.unwrap_or(DEFAULT_MAX_UDP_PAYLOAD_SIZE)
    }

    /// Effective `ack_delay_exponent`, applying the RFC 9000 §18.2 default of
    /// [`DEFAULT_ACK_DELAY_EXPONENT`] when the parameter is absent.
    #[must_use]
    pub fn ack_delay_exponent(&self) -> u64 {
        self.ack_delay_exponent.unwrap_or(DEFAULT_ACK_DELAY_EXPONENT)
    }

    /// Effective `max_ack_delay` in milliseconds, applying the RFC 9000 §18.2
    /// default of [`DEFAULT_MAX_ACK_DELAY_MS`] when the parameter is absent.
    #[must_use]
    pub fn max_ack_delay_ms(&self) -> u64 {
        self.max_ack_delay_ms.unwrap_or(DEFAULT_MAX_ACK_DELAY_MS)
    }

    /// Effective `active_connection_id_limit`, applying the RFC 9000 §18.2
    /// default of [`MIN_ACTIVE_CONNECTION_ID_LIMIT`] when the parameter is absent.
    #[must_use]
    pub fn active_connection_id_limit(&self) -> u64 {
        self.active_connection_id_limit.unwrap_or(MIN_ACTIVE_CONNECTION_ID_LIMIT)
    }

    /// Parse the body of a `quic_transport_parameters` extension (RFC 9000 §18):
    /// a concatenation of `(id, length, value)` entries with no outer framing.
    ///
    /// # Errors
    ///
    /// Returns a [`TransportParameterError`] on a truncated entry, a duplicate
    /// id (RFC 9000 §7.4), a value whose shape or range violates RFC 9000 §18.2,
    /// or a varint that exceeds the QUIC maximum.
    pub fn parse(mut input: &[u8]) -> Result<Self, TransportParameterError> {
        let mut params = TransportParameters::default();
        // Track which ids were already seen to reject duplicates (RFC 9000 §7.4).
        let mut seen: Vec<u64> = Vec::new();
        while !input.is_empty() {
            let id = take_varint(&mut input)?;
            let len = take_varint(&mut input)?;
            let len = usize::try_from(len).map_err(|_| TransportParameterError::LengthTooLong)?;
            if input.len() < len {
                return Err(TransportParameterError::LengthTooLong);
            }
            let (value, rest) = input.split_at(len);
            input = rest;
            if seen.contains(&id) {
                return Err(TransportParameterError::DuplicateParameter(id));
            }
            seen.push(id);
            params.apply(id, value)?;
        }
        Ok(params)
    }

    /// Route one decoded `(id, value)` entry into the appropriate field,
    /// validating it per RFC 9000 §18.2.
    fn apply(&mut self, id: u64, value: &[u8]) -> Result<(), TransportParameterError> {
        match id {
            PARAM_ORIGINAL_DESTINATION_CONNECTION_ID => {
                self.original_destination_connection_id = Some(value.to_vec());
            }
            PARAM_MAX_IDLE_TIMEOUT => {
                self.max_idle_timeout_ms = Some(int_value(id, value)?);
            }
            PARAM_STATELESS_RESET_TOKEN => {
                self.stateless_reset_token = Some(fixed_value::<STATELESS_RESET_TOKEN_LEN>(id, value)?);
            }
            PARAM_MAX_UDP_PAYLOAD_SIZE => {
                let v = int_value(id, value)?;
                if v < MIN_MAX_UDP_PAYLOAD_SIZE {
                    return Err(TransportParameterError::InvalidValue(id));
                }
                self.max_udp_payload_size = Some(v);
            }
            PARAM_INITIAL_MAX_DATA => self.initial_max_data = Some(int_value(id, value)?),
            PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL => {
                self.initial_max_stream_data_bidi_local = Some(int_value(id, value)?);
            }
            PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE => {
                self.initial_max_stream_data_bidi_remote = Some(int_value(id, value)?);
            }
            PARAM_INITIAL_MAX_STREAM_DATA_UNI => {
                self.initial_max_stream_data_uni = Some(int_value(id, value)?);
            }
            PARAM_INITIAL_MAX_STREAMS_BIDI => {
                self.initial_max_streams_bidi = Some(int_value(id, value)?);
            }
            PARAM_INITIAL_MAX_STREAMS_UNI => {
                self.initial_max_streams_uni = Some(int_value(id, value)?);
            }
            PARAM_ACK_DELAY_EXPONENT => {
                let v = int_value(id, value)?;
                if v > MAX_ACK_DELAY_EXPONENT {
                    return Err(TransportParameterError::InvalidValue(id));
                }
                self.ack_delay_exponent = Some(v);
            }
            PARAM_MAX_ACK_DELAY => {
                let v = int_value(id, value)?;
                if v >= MAX_ACK_DELAY_LIMIT_MS {
                    return Err(TransportParameterError::InvalidValue(id));
                }
                self.max_ack_delay_ms = Some(v);
            }
            PARAM_DISABLE_ACTIVE_MIGRATION => {
                if !value.is_empty() {
                    return Err(TransportParameterError::MalformedValue(id));
                }
                self.disable_active_migration = true;
            }
            PARAM_PREFERRED_ADDRESS => {
                self.preferred_address = Some(PreferredAddress::parse(value)?);
            }
            PARAM_ACTIVE_CONNECTION_ID_LIMIT => {
                let v = int_value(id, value)?;
                if v < MIN_ACTIVE_CONNECTION_ID_LIMIT {
                    return Err(TransportParameterError::InvalidValue(id));
                }
                self.active_connection_id_limit = Some(v);
            }
            PARAM_INITIAL_SOURCE_CONNECTION_ID => {
                self.initial_source_connection_id = Some(value.to_vec());
            }
            PARAM_RETRY_SOURCE_CONNECTION_ID => {
                self.retry_source_connection_id = Some(value.to_vec());
            }
            // Unknown or reserved (GREASE) parameter — preserve and ignore
            // (RFC 9000 §18.1).
            _ => self.unknown.push((id, value.to_vec())),
        }
        Ok(())
    }

    /// Serialize these transport parameters into an extension body (RFC 9000
    /// §18). Known parameters are written in ascending id order, then the
    /// preserved unknown parameters in their stored order.
    ///
    /// # Errors
    ///
    /// Returns [`TransportParameterError::VarIntOverflow`] if any integer value
    /// exceeds the QUIC varint maximum, or
    /// [`TransportParameterError::MalformedValue`] if a preferred address holds
    /// an over-long connection id.
    pub fn serialize(&self) -> Result<Vec<u8>, TransportParameterError> {
        let mut out = Vec::new();
        if let Some(cid) = &self.original_destination_connection_id {
            put_bytes_param(PARAM_ORIGINAL_DESTINATION_CONNECTION_ID, cid, &mut out)?;
        }
        if let Some(v) = self.max_idle_timeout_ms {
            put_int_param(PARAM_MAX_IDLE_TIMEOUT, v, &mut out)?;
        }
        if let Some(token) = &self.stateless_reset_token {
            put_bytes_param(PARAM_STATELESS_RESET_TOKEN, token, &mut out)?;
        }
        if let Some(v) = self.max_udp_payload_size {
            put_int_param(PARAM_MAX_UDP_PAYLOAD_SIZE, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_data {
            put_int_param(PARAM_INITIAL_MAX_DATA, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_stream_data_bidi_local {
            put_int_param(PARAM_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_stream_data_bidi_remote {
            put_int_param(PARAM_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_stream_data_uni {
            put_int_param(PARAM_INITIAL_MAX_STREAM_DATA_UNI, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_streams_bidi {
            put_int_param(PARAM_INITIAL_MAX_STREAMS_BIDI, v, &mut out)?;
        }
        if let Some(v) = self.initial_max_streams_uni {
            put_int_param(PARAM_INITIAL_MAX_STREAMS_UNI, v, &mut out)?;
        }
        if let Some(v) = self.ack_delay_exponent {
            put_int_param(PARAM_ACK_DELAY_EXPONENT, v, &mut out)?;
        }
        if let Some(v) = self.max_ack_delay_ms {
            put_int_param(PARAM_MAX_ACK_DELAY, v, &mut out)?;
        }
        if self.disable_active_migration {
            put_bytes_param(PARAM_DISABLE_ACTIVE_MIGRATION, &[], &mut out)?;
        }
        if let Some(addr) = &self.preferred_address {
            put_bytes_param(PARAM_PREFERRED_ADDRESS, &addr.serialize()?, &mut out)?;
        }
        if let Some(v) = self.active_connection_id_limit {
            put_int_param(PARAM_ACTIVE_CONNECTION_ID_LIMIT, v, &mut out)?;
        }
        if let Some(cid) = &self.initial_source_connection_id {
            put_bytes_param(PARAM_INITIAL_SOURCE_CONNECTION_ID, cid, &mut out)?;
        }
        if let Some(cid) = &self.retry_source_connection_id {
            put_bytes_param(PARAM_RETRY_SOURCE_CONNECTION_ID, cid, &mut out)?;
        }
        for (id, value) in &self.unknown {
            put_bytes_param(*id, value, &mut out)?;
        }
        Ok(out)
    }
}

// ── Value decoding helpers ───────────────────────────────────────────────────

/// Decode an integer-valued parameter: a single varint that must fill the
/// entire parameter value exactly (RFC 9000 §18.1).
fn int_value(id: u64, value: &[u8]) -> Result<u64, TransportParameterError> {
    let (v, consumed) = varint::decode(value).ok_or(TransportParameterError::MalformedValue(id))?;
    if consumed != value.len() {
        return Err(TransportParameterError::MalformedValue(id));
    }
    Ok(v)
}

/// Decode a fixed-width `N`-byte parameter value.
fn fixed_value<const N: usize>(id: u64, value: &[u8]) -> Result<[u8; N], TransportParameterError> {
    if value.len() != N {
        return Err(TransportParameterError::MalformedValue(id));
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(value);
    Ok(arr)
}

// ── Serialization helpers ────────────────────────────────────────────────────

/// Append an integer parameter (`id`, varint-encoded `value`) to `out`.
fn put_int_param(id: u64, value: u64, out: &mut Vec<u8>) -> Result<(), TransportParameterError> {
    varint::encode(id, out)?;
    // Length is the encoded width of the value varint.
    let len = varint::encoded_len(value).ok_or(TransportParameterError::VarIntOverflow(value))?;
    varint::encode(len as u64, out)?;
    varint::encode(value, out)?;
    Ok(())
}

/// Append a byte-string parameter (`id`, length-prefixed `value`) to `out`.
fn put_bytes_param(id: u64, value: &[u8], out: &mut Vec<u8>) -> Result<(), TransportParameterError> {
    varint::encode(id, out)?;
    varint::encode(value.len() as u64, out)?;
    out.extend_from_slice(value);
    Ok(())
}

// ── Slice-reading helpers ────────────────────────────────────────────────────

/// Pull one varint off the front of `buf`, advancing it.
fn take_varint(buf: &mut &[u8]) -> Result<u64, TransportParameterError> {
    let (value, consumed) =
        varint::decode(buf).ok_or(TransportParameterError::UnexpectedEof)?;
    *buf = &buf[consumed..];
    Ok(value)
}

/// Pull a single byte off the front of `buf`, advancing it; `id` labels the
/// enclosing parameter for the error.
fn take_u8(buf: &mut &[u8], id: u64) -> Result<u8, TransportParameterError> {
    let (&first, rest) = buf.split_first().ok_or(TransportParameterError::MalformedValue(id))?;
    *buf = rest;
    Ok(first)
}

/// Pull exactly `n` bytes off the front of `buf`, advancing it; `id` labels the
/// enclosing parameter for the error.
fn take_bytes<'a>(
    buf: &mut &'a [u8],
    n: usize,
    id: u64,
) -> Result<&'a [u8], TransportParameterError> {
    if buf.len() < n {
        return Err(TransportParameterError::MalformedValue(id));
    }
    let (head, rest) = buf.split_at(n);
    *buf = rest;
    Ok(head)
}

/// Pull a fixed-size `N`-byte array off the front of `buf`, advancing it; `id`
/// labels the enclosing parameter for the error.
fn take_array<const N: usize>(
    buf: &mut &[u8],
    id: u64,
) -> Result<[u8; N], TransportParameterError> {
    let bytes = take_bytes(buf, N, id)?;
    let mut arr = [0u8; N];
    arr.copy_from_slice(bytes);
    Ok(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully-populated set of client-style parameters round-trips byte-stably.
    #[test]
    fn roundtrip_full_client_params() {
        let params = TransportParameters {
            max_idle_timeout_ms: Some(30_000),
            max_udp_payload_size: Some(1472),
            initial_max_data: Some(1_048_576),
            initial_max_stream_data_bidi_local: Some(262_144),
            initial_max_stream_data_bidi_remote: Some(262_144),
            initial_max_stream_data_uni: Some(131_072),
            initial_max_streams_bidi: Some(100),
            initial_max_streams_uni: Some(3),
            ack_delay_exponent: Some(3),
            max_ack_delay_ms: Some(25),
            disable_active_migration: true,
            active_connection_id_limit: Some(4),
            initial_source_connection_id: Some(vec![0xde, 0xad, 0xbe, 0xef]),
            ..Default::default()
        };
        let wire = params.serialize().expect("serialize");
        let parsed = TransportParameters::parse(&wire).expect("parse");
        assert_eq!(parsed, params);
    }

    /// Absent parameters resolve to their RFC 9000 §18.2 defaults.
    #[test]
    fn defaults_when_absent() {
        let params = TransportParameters::default();
        assert_eq!(params.max_udp_payload_size(), DEFAULT_MAX_UDP_PAYLOAD_SIZE);
        assert_eq!(params.ack_delay_exponent(), DEFAULT_ACK_DELAY_EXPONENT);
        assert_eq!(params.max_ack_delay_ms(), DEFAULT_MAX_ACK_DELAY_MS);
        assert_eq!(params.active_connection_id_limit(), MIN_ACTIVE_CONNECTION_ID_LIMIT);
        // Serializing the empty set yields an empty extension body.
        assert!(params.serialize().unwrap().is_empty());
        assert_eq!(TransportParameters::parse(&[]).unwrap(), params);
    }

    /// A hand-built wire sequence decodes field-by-field (RFC 9000 §18 layout).
    #[test]
    fn parses_known_wire_layout() {
        // id=0x04 (initial_max_data), len=0x02, value=0x4400 (varint 0x0400=1024)
        // id=0x08 (initial_max_streams_bidi), len=0x01, value=0x0a (10)
        let wire = [0x04, 0x02, 0x44, 0x00, 0x08, 0x01, 0x0a];
        let p = TransportParameters::parse(&wire).unwrap();
        assert_eq!(p.initial_max_data, Some(1024));
        assert_eq!(p.initial_max_streams_bidi, Some(10));
        assert_eq!(p.serialize().unwrap(), wire);
    }

    /// A duplicate parameter id is a connection error (RFC 9000 §7.4).
    #[test]
    fn rejects_duplicate_parameter() {
        // initial_max_data twice.
        let wire = [0x04, 0x01, 0x0a, 0x04, 0x01, 0x0b];
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::DuplicateParameter(PARAM_INITIAL_MAX_DATA))
        );
    }

    /// An integer value that does not fill its declared Length is malformed
    /// (RFC 9000 §18.1).
    #[test]
    fn rejects_integer_value_not_filling_length() {
        // initial_max_data, Length=2 but a 1-byte varint value plus a stray byte.
        let wire = [0x04, 0x02, 0x0a, 0x00];
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::MalformedValue(PARAM_INITIAL_MAX_DATA))
        );
    }

    /// `max_udp_payload_size` below the 1200 floor is out of range.
    #[test]
    fn rejects_small_max_udp_payload_size() {
        let mut out = Vec::new();
        put_int_param(PARAM_MAX_UDP_PAYLOAD_SIZE, 1199, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out),
            Err(TransportParameterError::InvalidValue(PARAM_MAX_UDP_PAYLOAD_SIZE))
        );
        // Exactly the floor is accepted.
        out.clear();
        put_int_param(PARAM_MAX_UDP_PAYLOAD_SIZE, MIN_MAX_UDP_PAYLOAD_SIZE, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out).unwrap().max_udp_payload_size(),
            MIN_MAX_UDP_PAYLOAD_SIZE
        );
    }

    /// `ack_delay_exponent` above 20 and `max_ack_delay` at/above 2^14 are
    /// out of range (RFC 9000 §18.2).
    #[test]
    fn rejects_out_of_range_ack_params() {
        let mut out = Vec::new();
        put_int_param(PARAM_ACK_DELAY_EXPONENT, MAX_ACK_DELAY_EXPONENT + 1, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out),
            Err(TransportParameterError::InvalidValue(PARAM_ACK_DELAY_EXPONENT))
        );
        out.clear();
        put_int_param(PARAM_MAX_ACK_DELAY, MAX_ACK_DELAY_LIMIT_MS, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out),
            Err(TransportParameterError::InvalidValue(PARAM_MAX_ACK_DELAY))
        );
        // 2^14 - 1 is the largest accepted value.
        out.clear();
        put_int_param(PARAM_MAX_ACK_DELAY, MAX_ACK_DELAY_LIMIT_MS - 1, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out).unwrap().max_ack_delay_ms(),
            MAX_ACK_DELAY_LIMIT_MS - 1
        );
    }

    /// `active_connection_id_limit` below 2 is out of range (RFC 9000 §18.2).
    #[test]
    fn rejects_small_active_cid_limit() {
        let mut out = Vec::new();
        put_int_param(PARAM_ACTIVE_CONNECTION_ID_LIMIT, 1, &mut out).unwrap();
        assert_eq!(
            TransportParameters::parse(&out),
            Err(TransportParameterError::InvalidValue(PARAM_ACTIVE_CONNECTION_ID_LIMIT))
        );
    }

    /// A stateless reset token must be exactly 16 bytes.
    #[test]
    fn rejects_wrong_stateless_reset_token_len() {
        // Length 15 for the token.
        let mut wire = vec![PARAM_STATELESS_RESET_TOKEN as u8, 15];
        wire.extend_from_slice(&[0u8; 15]);
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::MalformedValue(PARAM_STATELESS_RESET_TOKEN))
        );
        // Exactly 16 bytes round-trips.
        let params = TransportParameters {
            stateless_reset_token: Some([0xab; STATELESS_RESET_TOKEN_LEN]),
            ..Default::default()
        };
        let out = params.serialize().unwrap();
        assert_eq!(TransportParameters::parse(&out).unwrap(), params);
    }

    /// `disable_active_migration` must be zero-length; a non-empty value is
    /// malformed.
    #[test]
    fn rejects_nonempty_disable_active_migration() {
        let wire = [PARAM_DISABLE_ACTIVE_MIGRATION as u8, 0x01, 0x00];
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::MalformedValue(PARAM_DISABLE_ACTIVE_MIGRATION))
        );
    }

    /// A preferred address round-trips through its typed struct.
    #[test]
    fn roundtrip_preferred_address() {
        let addr = PreferredAddress {
            ipv4: [93, 184, 216, 34],
            ipv4_port: 443,
            ipv6: [
                0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
            ],
            ipv6_port: 443,
            connection_id: vec![0x01, 0x02, 0x03, 0x04, 0x05],
            stateless_reset_token: [0x9a; STATELESS_RESET_TOKEN_LEN],
        };
        let params = TransportParameters {
            preferred_address: Some(addr.clone()),
            ..Default::default()
        };
        let wire = params.serialize().unwrap();
        assert_eq!(TransportParameters::parse(&wire).unwrap().preferred_address, Some(addr));
    }

    /// A preferred address whose embedded connection-id length exceeds 20 is
    /// malformed.
    #[test]
    fn rejects_overlong_preferred_address_cid() {
        // ipv4(4) + port(2) + ipv6(16) + port(2) + cid_len(1)=21 + cid(21) + token(16)
        let mut value = Vec::new();
        value.extend_from_slice(&[0u8; 4]);
        value.extend_from_slice(&0u16.to_be_bytes());
        value.extend_from_slice(&[0u8; 16]);
        value.extend_from_slice(&0u16.to_be_bytes());
        value.push(21); // > MAX_CONNECTION_ID_LEN
        value.extend_from_slice(&[0u8; 21]);
        value.extend_from_slice(&[0u8; 16]);
        let mut wire = vec![PARAM_PREFERRED_ADDRESS as u8, value.len() as u8];
        wire.extend_from_slice(&value);
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::MalformedValue(PARAM_PREFERRED_ADDRESS))
        );
    }

    /// Unknown / GREASE parameters are preserved verbatim and round-trip.
    #[test]
    fn preserves_unknown_parameters() {
        // Reserved id of the form 31*N+27 (RFC 9000 §18.1): 27 (N=0), 58 (N=1).
        let wire = [0x1b, 0x02, 0xaa, 0xbb, 0x3a, 0x00];
        let p = TransportParameters::parse(&wire).unwrap();
        assert_eq!(p.unknown, vec![(0x1b, vec![0xaa, 0xbb]), (0x3a, vec![])]);
        assert_eq!(p.serialize().unwrap(), wire);
    }

    /// A parameter whose Length runs past the buffer is a length error.
    #[test]
    fn rejects_length_past_end() {
        let wire = [0x04, 0x05, 0x00, 0x00]; // Length 5, only 2 bytes present
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::LengthTooLong)
        );
    }

    /// Truncation in the middle of an id/length varint is an EOF error.
    #[test]
    fn rejects_truncated_header() {
        // A two-byte varint id prefix (0x40..) with only one byte present.
        let wire = [0x40];
        assert_eq!(
            TransportParameters::parse(&wire),
            Err(TransportParameterError::UnexpectedEof)
        );
    }

    /// Every codec error maps to TRANSPORT_PARAMETER_ERROR (RFC 9000 §20.1).
    #[test]
    fn all_errors_map_to_transport_parameter_error() {
        for e in [
            TransportParameterError::UnexpectedEof,
            TransportParameterError::LengthTooLong,
            TransportParameterError::VarIntOverflow(0),
            TransportParameterError::DuplicateParameter(0x04),
            TransportParameterError::MalformedValue(0x04),
            TransportParameterError::InvalidValue(0x04),
        ] {
            assert_eq!(e.code(), TRANSPORT_PARAMETER_ERROR);
        }
    }

    /// The largest-varint value survives a round-trip (8-byte encoding path).
    #[test]
    fn roundtrip_max_varint_value() {
        let params = TransportParameters {
            initial_max_data: Some(varint::MAX_VARINT),
            ..Default::default()
        };
        let wire = params.serialize().unwrap();
        assert_eq!(TransportParameters::parse(&wire).unwrap(), params);
    }
}
