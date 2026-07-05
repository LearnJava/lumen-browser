//! HTTP/3 connection SETTINGS (RFC 9114 §7.2.4, RFC 9204 §5, RFC 9220 §3).
//!
//! The typed bridge between the raw `(identifier, value)` SETTINGS pairs of the
//! frame codec ([`crate::h3::frame::Frame::Settings`]) and the policy layers
//! that consume them — the QPACK encoder ([`crate::h3::qpack_encoder`], which
//! reads the peer's advertised dynamic-table capacity and blocked-stream budget)
//! and the request path ([`crate::h3::h3_request`], which honours the peer's
//! maximum field-section size). This mirrors the HTTP/2 counterpart
//! [`crate::http::h2_settings::H2Settings`].
//!
//! [`H3Settings::for_profile`] builds the local SETTINGS Lumen sends on its
//! control stream, ordered to match the impersonated browser's fingerprint
//! (SETTINGS values and order are observable on the wire and anti-bot layers key
//! on them). [`H3Settings::from_pairs`] parses the peer's SETTINGS into typed
//! values, applying the RFC defaults for any absent setting, validating the
//! value ranges the codec cannot check locally, and preserving the frame codec's
//! two connection-level rules (reserved HTTP/2 identifiers and duplicate
//! identifiers are `H3_SETTINGS_ERROR`) so it is robust regardless of whether
//! its pairs came through the frame codec. Unknown / greased identifiers
//! (RFC 9114 §7.2.4.2) are ignored. Pure, no IO — the control-stream framing and
//! the SETTINGS-before-request sequencing live with [`crate::h3::h3_stream`].

use crate::h3::frame::{
    Frame, FrameError, SETTING_ENABLE_CONNECT_PROTOCOL, SETTING_MAX_FIELD_SECTION_SIZE,
    SETTING_QPACK_BLOCKED_STREAMS, SETTING_QPACK_MAX_TABLE_CAPACITY, H3_SETTINGS_ERROR,
};
use crate::h3::h3_request::H3Profile;

/// An error interpreting a peer's HTTP/3 SETTINGS.
///
/// Every variant maps to `H3_SETTINGS_ERROR` (RFC 9114 §8.1) — the connection
/// error an endpoint raises for invalid SETTINGS content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum H3SettingsError {
    /// A reserved HTTP/2 setting identifier (`0x00`, `0x02`–`0x05`) appeared in
    /// an HTTP/3 SETTINGS frame (RFC 9114 §7.2.4.1).
    ReservedIdentifier(u64),
    /// The same setting identifier occurred more than once (RFC 9114 §7.2.4).
    DuplicateIdentifier(u64),
    /// A known setting carried a value outside its permitted range — currently
    /// only `SETTINGS_ENABLE_CONNECT_PROTOCOL`, which must be `0` or `1`
    /// (RFC 9220 §3).
    InvalidValue {
        /// The setting identifier whose value was rejected.
        id: u64,
        /// The offending value.
        value: u64,
    },
    /// [`H3Settings::from_frame`] was given a frame that is not a SETTINGS frame.
    NotSettings,
}

impl H3SettingsError {
    /// The RFC 9114 §8.1 error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        H3_SETTINGS_ERROR
    }
}

impl core::fmt::Display for H3SettingsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ReservedIdentifier(id) => {
                write!(f, "H3_SETTINGS_ERROR: reserved HTTP/2 setting 0x{id:02x}")
            }
            Self::DuplicateIdentifier(id) => {
                write!(f, "H3_SETTINGS_ERROR: duplicate setting 0x{id:x}")
            }
            Self::InvalidValue { id, value } => {
                write!(f, "H3_SETTINGS_ERROR: setting 0x{id:x} has invalid value {value}")
            }
            Self::NotSettings => write!(f, "H3_SETTINGS_ERROR: frame is not SETTINGS"),
        }
    }
}

impl std::error::Error for H3SettingsError {}

/// The typed contents of an HTTP/3 SETTINGS frame (RFC 9114 §7.2.4).
///
/// Field values are the effective settings after RFC defaults are applied for
/// any absent identifier, so [`H3Settings::default`] is exactly the behaviour of
/// a peer that sent an empty SETTINGS frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H3Settings {
    /// `SETTINGS_QPACK_MAX_TABLE_CAPACITY` (RFC 9204 §5) — the maximum size in
    /// bytes the peer permits for the QPACK dynamic table it decodes. Default
    /// `0`, meaning the peer will not use the dynamic table (static table only).
    pub qpack_max_table_capacity: u64,
    /// `SETTINGS_QPACK_BLOCKED_STREAMS` (RFC 9204 §5) — how many streams the peer
    /// tolerates being blocked on a QPACK dynamic-table insertion it has not yet
    /// received. Default `0`, meaning the encoder must never reference an
    /// unacknowledged entry.
    pub qpack_blocked_streams: u64,
    /// `SETTINGS_MAX_FIELD_SECTION_SIZE` (RFC 9114 §7.2.4.1) — the maximum
    /// uncompressed size in bytes the peer accepts for a field section.
    /// `None` (the default) means the peer imposes no limit.
    pub max_field_section_size: Option<u64>,
    /// `SETTINGS_ENABLE_CONNECT_PROTOCOL` (RFC 9220 §3) — whether the peer
    /// supports the extended CONNECT method used to bootstrap WebTransport /
    /// WebSocket-over-HTTP/3. Default `false`.
    pub enable_connect_protocol: bool,
}

impl Default for H3Settings {
    /// The RFC defaults — the effective settings of a peer that sent an empty
    /// SETTINGS frame.
    fn default() -> Self {
        Self {
            qpack_max_table_capacity: 0,
            qpack_blocked_streams: 0,
            max_field_section_size: None,
            enable_connect_protocol: false,
        }
    }
}

impl H3Settings {
    /// Build the local SETTINGS Lumen sends on its control stream for the given
    /// impersonation profile.
    ///
    /// The values match the QUIC/HTTP/3 stack of the impersonated browser so the
    /// SETTINGS frame does not stand out as a fingerprint (per ADR-007
    /// «Per-profile HTTP configs»). Chrome/Edge and Firefox both enable a
    /// 64 KiB QPACK dynamic table; Safari is conservative. None of the profiles
    /// advertise a field-section-size limit or the CONNECT protocol.
    #[must_use]
    pub fn for_profile(profile: H3Profile) -> Self {
        match profile {
            // Chrome / Edge (quiche) — 64 KiB table, 100 blocked streams.
            H3Profile::Chrome => Self {
                qpack_max_table_capacity: 65536,
                qpack_blocked_streams: 100,
                max_field_section_size: None,
                enable_connect_protocol: false,
            },
            // Firefox — 64 KiB table, a smaller blocked-stream budget.
            H3Profile::Firefox => Self {
                qpack_max_table_capacity: 65536,
                qpack_blocked_streams: 20,
                max_field_section_size: None,
                enable_connect_protocol: false,
            },
            // Safari — conservative 4 KiB table.
            H3Profile::Safari => Self {
                qpack_max_table_capacity: 4096,
                qpack_blocked_streams: 100,
                max_field_section_size: None,
                enable_connect_protocol: false,
            },
        }
    }

    /// The `(identifier, value)` pairs this settings set serializes to, in the
    /// deterministic order Lumen transmits them.
    ///
    /// Only settings that differ from their RFC default are emitted: a QPACK
    /// capacity or blocked-stream budget of `0`, an absent field-section limit,
    /// and a disabled CONNECT protocol are all the wire default and are left off
    /// to keep the frame minimal (a real browser omits settings it leaves at the
    /// default). The order — QPACK max table capacity, QPACK blocked streams,
    /// max field-section size, enable CONNECT protocol — is stable so the frame
    /// bytes are reproducible.
    #[must_use]
    pub fn to_pairs(&self) -> Vec<(u64, u64)> {
        let mut pairs = Vec::new();
        if self.qpack_max_table_capacity != 0 {
            pairs.push((SETTING_QPACK_MAX_TABLE_CAPACITY, self.qpack_max_table_capacity));
        }
        if self.qpack_blocked_streams != 0 {
            pairs.push((SETTING_QPACK_BLOCKED_STREAMS, self.qpack_blocked_streams));
        }
        if let Some(size) = self.max_field_section_size {
            pairs.push((SETTING_MAX_FIELD_SECTION_SIZE, size));
        }
        if self.enable_connect_protocol {
            pairs.push((SETTING_ENABLE_CONNECT_PROTOCOL, 1));
        }
        pairs
    }

    /// Build the SETTINGS frame Lumen sends as the first frame on its control
    /// stream (RFC 9114 §6.2.1).
    #[must_use]
    pub fn to_frame(&self) -> Frame {
        Frame::Settings(self.to_pairs())
    }

    /// Serialize the SETTINGS frame to its wire bytes.
    ///
    /// # Errors
    ///
    /// [`FrameError`] if a setting value overflows the varint encoding — which
    /// cannot happen for the profile-derived settings, but is surfaced for
    /// caller-constructed values.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let mut buf = Vec::new();
        self.to_frame().encode(&mut buf)?;
        Ok(buf)
    }

    /// Interpret a peer's SETTINGS `(identifier, value)` pairs into typed
    /// settings, applying the RFC default for every absent identifier.
    ///
    /// This re-enforces the two connection-level rules of RFC 9114 §7.2.4.1
    /// (reserved HTTP/2 identifiers and duplicate identifiers) so the typed
    /// layer is self-contained even when its pairs did not come through the
    /// frame codec, validates the `SETTINGS_ENABLE_CONNECT_PROTOCOL` value
    /// range (RFC 9220 §3), and ignores unknown / greased identifiers
    /// (RFC 9114 §7.2.4.2).
    ///
    /// # Errors
    ///
    /// [`H3SettingsError`] for a reserved or duplicate identifier, or an
    /// out-of-range value — each mapping to `H3_SETTINGS_ERROR`.
    pub fn from_pairs(pairs: &[(u64, u64)]) -> Result<Self, H3SettingsError> {
        let mut settings = Self::default();
        // A small inline seen-set: the number of distinct settings is tiny, so a
        // linear scan beats a HashSet allocation.
        let mut seen: Vec<u64> = Vec::with_capacity(pairs.len());
        for &(id, value) in pairs {
            // 0x00 and 0x02–0x05 were HTTP/2 settings; their presence in an
            // HTTP/3 SETTINGS frame is a connection error (RFC 9114 §7.2.4.1).
            if matches!(id, 0x00 | 0x02 | 0x03 | 0x04 | 0x05) {
                return Err(H3SettingsError::ReservedIdentifier(id));
            }
            if seen.contains(&id) {
                return Err(H3SettingsError::DuplicateIdentifier(id));
            }
            seen.push(id);

            match id {
                SETTING_QPACK_MAX_TABLE_CAPACITY => settings.qpack_max_table_capacity = value,
                SETTING_QPACK_BLOCKED_STREAMS => settings.qpack_blocked_streams = value,
                SETTING_MAX_FIELD_SECTION_SIZE => settings.max_field_section_size = Some(value),
                SETTING_ENABLE_CONNECT_PROTOCOL => match value {
                    0 => settings.enable_connect_protocol = false,
                    1 => settings.enable_connect_protocol = true,
                    _ => return Err(H3SettingsError::InvalidValue { id, value }),
                },
                // Unknown / greased identifier — RFC 9114 §7.2.4.2 requires it to
                // be ignored.
                _ => {}
            }
        }
        Ok(settings)
    }

    /// Interpret a peer's decoded SETTINGS frame into typed settings.
    ///
    /// # Errors
    ///
    /// [`H3SettingsError::NotSettings`] if `frame` is not a
    /// [`Frame::Settings`], otherwise the same errors as [`Self::from_pairs`].
    pub fn from_frame(frame: &Frame) -> Result<Self, H3SettingsError> {
        match frame {
            Frame::Settings(pairs) => Self::from_pairs(pairs),
            _ => Err(H3SettingsError::NotSettings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_frame_behaviour() {
        let s = H3Settings::default();
        assert_eq!(s.qpack_max_table_capacity, 0);
        assert_eq!(s.qpack_blocked_streams, 0);
        assert_eq!(s.max_field_section_size, None);
        assert!(!s.enable_connect_protocol);
        // An empty SETTINGS frame parses to the defaults.
        assert_eq!(H3Settings::from_pairs(&[]).unwrap(), s);
    }

    #[test]
    fn chrome_profile_values() {
        let s = H3Settings::for_profile(H3Profile::Chrome);
        assert_eq!(s.qpack_max_table_capacity, 65536);
        assert_eq!(s.qpack_blocked_streams, 100);
        assert_eq!(s.max_field_section_size, None);
        assert!(!s.enable_connect_protocol);
    }

    #[test]
    fn firefox_profile_values() {
        let s = H3Settings::for_profile(H3Profile::Firefox);
        assert_eq!(s.qpack_max_table_capacity, 65536);
        assert_eq!(s.qpack_blocked_streams, 20);
    }

    #[test]
    fn safari_profile_conservative() {
        let s = H3Settings::for_profile(H3Profile::Safari);
        assert_eq!(s.qpack_max_table_capacity, 4096);
        assert_eq!(s.qpack_blocked_streams, 100);
    }

    #[test]
    fn to_pairs_omits_defaults() {
        // A settings set at all defaults emits nothing on the wire.
        assert!(H3Settings::default().to_pairs().is_empty());
    }

    #[test]
    fn to_pairs_order_is_stable() {
        let s = H3Settings {
            qpack_max_table_capacity: 65536,
            qpack_blocked_streams: 100,
            max_field_section_size: Some(0x4000),
            enable_connect_protocol: true,
        };
        assert_eq!(
            s.to_pairs(),
            vec![
                (SETTING_QPACK_MAX_TABLE_CAPACITY, 65536),
                (SETTING_QPACK_BLOCKED_STREAMS, 100),
                (SETTING_MAX_FIELD_SECTION_SIZE, 0x4000),
                (SETTING_ENABLE_CONNECT_PROTOCOL, 1),
            ]
        );
    }

    #[test]
    fn round_trip_through_frame() {
        let s = H3Settings::for_profile(H3Profile::Chrome);
        let frame = s.to_frame();
        // Re-parsing our own frame reproduces the settings exactly (the fields we
        // emit are non-default, so nothing is lost to default-omission).
        assert_eq!(H3Settings::from_frame(&frame).unwrap(), s);
    }

    #[test]
    fn round_trip_through_wire_bytes() {
        let s = H3Settings::for_profile(H3Profile::Safari);
        let bytes = s.encode().unwrap();
        let (frame, consumed) = Frame::parse(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(H3Settings::from_frame(&frame).unwrap(), s);
    }

    #[test]
    fn encode_matches_frame_codec() {
        let s = H3Settings::for_profile(H3Profile::Firefox);
        let mut expected = Vec::new();
        Frame::Settings(s.to_pairs()).encode(&mut expected).unwrap();
        assert_eq!(s.encode().unwrap(), expected);
    }

    #[test]
    fn parses_all_known_settings() {
        let s = H3Settings::from_pairs(&[
            (SETTING_QPACK_MAX_TABLE_CAPACITY, 4096),
            (SETTING_QPACK_BLOCKED_STREAMS, 16),
            (SETTING_MAX_FIELD_SECTION_SIZE, 0x8000),
            (SETTING_ENABLE_CONNECT_PROTOCOL, 1),
        ])
        .unwrap();
        assert_eq!(s.qpack_max_table_capacity, 4096);
        assert_eq!(s.qpack_blocked_streams, 16);
        assert_eq!(s.max_field_section_size, Some(0x8000));
        assert!(s.enable_connect_protocol);
    }

    #[test]
    fn unknown_settings_are_ignored() {
        // A greased identifier (RFC 9114 §7.2.4.2) plus a known one: the greased
        // one is ignored, the known one is applied.
        let s = H3Settings::from_pairs(&[
            (0x1f * 31 + 0x21, 0xdead), // arbitrary greased id
            (SETTING_QPACK_MAX_TABLE_CAPACITY, 512),
        ])
        .unwrap();
        assert_eq!(s.qpack_max_table_capacity, 512);
    }

    #[test]
    fn reserved_h2_identifier_rejected() {
        for id in [0x00u64, 0x02, 0x03, 0x04, 0x05] {
            let err = H3Settings::from_pairs(&[(id, 1)]).unwrap_err();
            assert_eq!(err, H3SettingsError::ReservedIdentifier(id));
            assert_eq!(err.code(), H3_SETTINGS_ERROR);
        }
    }

    #[test]
    fn duplicate_identifier_rejected() {
        let err = H3Settings::from_pairs(&[
            (SETTING_QPACK_MAX_TABLE_CAPACITY, 1),
            (SETTING_QPACK_MAX_TABLE_CAPACITY, 2),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            H3SettingsError::DuplicateIdentifier(SETTING_QPACK_MAX_TABLE_CAPACITY)
        );
    }

    #[test]
    fn duplicate_unknown_identifier_rejected() {
        // The duplicate rule applies to unknown identifiers too (RFC 9114 §7.2.4).
        let greased = 0x1f * 5 + 0x21;
        let err = H3Settings::from_pairs(&[(greased, 1), (greased, 2)]).unwrap_err();
        assert_eq!(err, H3SettingsError::DuplicateIdentifier(greased));
    }

    #[test]
    fn enable_connect_protocol_out_of_range_rejected() {
        let err = H3Settings::from_pairs(&[(SETTING_ENABLE_CONNECT_PROTOCOL, 2)]).unwrap_err();
        assert_eq!(
            err,
            H3SettingsError::InvalidValue {
                id: SETTING_ENABLE_CONNECT_PROTOCOL,
                value: 2,
            }
        );
        assert_eq!(err.code(), H3_SETTINGS_ERROR);
    }

    #[test]
    fn enable_connect_protocol_zero_is_false() {
        let s = H3Settings::from_pairs(&[(SETTING_ENABLE_CONNECT_PROTOCOL, 0)]).unwrap();
        assert!(!s.enable_connect_protocol);
    }

    #[test]
    fn from_frame_rejects_non_settings() {
        let err = H3Settings::from_frame(&Frame::Data(vec![1, 2, 3])).unwrap_err();
        assert_eq!(err, H3SettingsError::NotSettings);
    }

    #[test]
    fn max_field_section_size_zero_is_present_not_absent() {
        // A value of 0 is a real advertised limit, distinct from an absent
        // setting (which is None / unlimited).
        let s = H3Settings::from_pairs(&[(SETTING_MAX_FIELD_SECTION_SIZE, 0)]).unwrap();
        assert_eq!(s.max_field_section_size, Some(0));
    }
}
