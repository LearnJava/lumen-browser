//! QUIC client-side version negotiation (RFC 9000 §6.2).
//!
//! When a client sends its first Initial packet with a QUIC version the server
//! does not support, the server answers with a Version Negotiation packet
//! ([`super::packet::Packet::VersionNegotiation`], codec'd in [`super::packet`]):
//! a long-header packet whose Version field is `0`, echoing the client's
//! connection IDs (RFC 9000 §6.1) and listing the versions the server supports,
//! in the server's preference order. The client picks a mutually supported
//! version and restarts the handshake with it, or abandons the attempt.
//!
//! This slice is the pure client-side state a client runs on a received Version
//! Negotiation packet, mirroring [`super::retry`]'s handling of a Retry:
//!
//! - [`VersionNegotiator`] holds the client's own supported versions (in
//!   descending preference order) and the version it attempted in its first
//!   Initial. [`VersionNegotiator::process`] validates the packet and reports
//!   the [`VersionNegotiationOutcome`] — the version to restart with — while
//!   enforcing the RFC 9000 §6.2 rules:
//!   - a client processes at most one Version Negotiation packet, and only
//!     before it has successfully processed any other packet from the server
//!     (a later Version Negotiation MUST be discarded);
//!   - a Version Negotiation packet that lists the version the client already
//!     attempted MUST be discarded (it is forged or erroneous — a genuine
//!     server would not answer a supported version with a Version Negotiation);
//!   - the client selects its most-preferred version that also appears in the
//!     server's list, and abandons the connection when the intersection is
//!     empty.
//!   - as an anti-injection sanity check the packet's Destination Connection ID
//!     must equal the client's Source Connection ID, and its Source Connection
//!     ID the client's Destination Connection ID (RFC 9000 §6.1).
//!
//! Pure functions and state; no IO. The caller drives it with a decoded
//! [`super::packet::Packet`] and, on success, re-derives its Initial keys for
//! the selected version and re-sends its Initial (RFC 9000 §6.2). Version
//! downgrade protection proper (echoing the negotiated version in the transport
//! parameters, RFC 9000 §6.3) lives with [`super::transport_params`].

use super::packet::Packet;

/// QUIC version 1 (RFC 9000 §15): the only version this client speaks.
pub const QUIC_VERSION_1: u32 = 0x0000_0001;

/// The wire value of the Version field that marks a Version Negotiation packet
/// (RFC 9000 §17.2.1): a long-header packet whose Version is `0`.
pub const VERSION_NEGOTIATION_VERSION: u32 = 0x0000_0000;

/// Failure processing a Version Negotiation packet (RFC 9000 §6.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionNegotiationError {
    /// The supplied [`Packet`] is not a [`Packet::VersionNegotiation`].
    NotVersionNegotiation,
    /// A Version Negotiation packet arrived after one was already processed
    /// (RFC 9000 §6.2): the client MUST discard it.
    AlreadyProcessed,
    /// The packet's echoed connection IDs do not match the ones the client sent
    /// (RFC 9000 §6.1): the packet is misdirected or injected and is discarded.
    ConnectionIdMismatch,
    /// The offered version list includes the version the client already
    /// attempted (RFC 9000 §6.2): a genuine server would not answer a supported
    /// version with a Version Negotiation, so the packet is forged or erroneous
    /// and MUST be discarded.
    DowngradeDetected,
    /// None of the client's supported versions appear in the server's list: the
    /// client abandons the connection attempt (RFC 9000 §6.2).
    NoCompatibleVersion,
}

impl core::fmt::Display for VersionNegotiationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VersionNegotiationError::NotVersionNegotiation => {
                write!(f, "packet is not a Version Negotiation packet")
            }
            VersionNegotiationError::AlreadyProcessed => {
                write!(f, "a Version Negotiation packet was already processed")
            }
            VersionNegotiationError::ConnectionIdMismatch => {
                write!(f, "Version Negotiation connection IDs do not match the sent packet")
            }
            VersionNegotiationError::DowngradeDetected => {
                write!(f, "Version Negotiation lists the version the client attempted")
            }
            VersionNegotiationError::NoCompatibleVersion => {
                write!(f, "no mutually supported QUIC version")
            }
        }
    }
}

impl std::error::Error for VersionNegotiationError {}

/// The result of processing a valid Version Negotiation packet (RFC 9000 §6.2):
/// the version the client restarts the handshake with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionNegotiationOutcome {
    /// The client's most-preferred version that the server also supports. The
    /// client re-derives its Initial keys for this version (RFC 9001 §5.2) and
    /// re-sends its Initial packet.
    pub selected_version: u32,
}

/// Client-side Version Negotiation state machine (RFC 9000 §6.2).
///
/// A client processes at most one Version Negotiation packet per connection, and
/// only as the first response from the server. [`VersionNegotiator`] records the
/// client's ordered version preferences and the version it attempted, verifies a
/// received packet, and reports the version to restart with.
#[derive(Clone, Debug)]
pub struct VersionNegotiator {
    /// The versions the client supports, in descending preference order.
    supported: Vec<u32>,
    /// The version the client used in its first Initial packet.
    attempted: u32,
    /// Whether a Version Negotiation packet has already been processed.
    processed: bool,
}

impl Default for VersionNegotiator {
    /// A negotiator that supports and attempted only [`QUIC_VERSION_1`].
    fn default() -> Self {
        Self { supported: vec![QUIC_VERSION_1], attempted: QUIC_VERSION_1, processed: false }
    }
}

impl VersionNegotiator {
    /// A negotiator that supports and attempted only [`QUIC_VERSION_1`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A negotiator with an explicit list of supported versions (in descending
    /// preference order) and the version the client attempted in its first
    /// Initial. The attempted version is expected to be the client's most
    /// preferred one, but that is the caller's policy, not enforced here.
    #[must_use]
    pub fn with_supported(supported: Vec<u32>, attempted: u32) -> Self {
        Self { supported, attempted, processed: false }
    }

    /// The version the client used in its first Initial packet.
    #[must_use]
    pub fn attempted_version(&self) -> u32 {
        self.attempted
    }

    /// Whether a Version Negotiation packet has already been processed on this
    /// connection.
    #[must_use]
    pub fn has_processed(&self) -> bool {
        self.processed
    }

    /// Select the client's most-preferred version that also appears in the
    /// server's `offered` list, or `None` if the intersection is empty. The
    /// client's preference order wins over the server's (RFC 9000 §6.2).
    #[must_use]
    fn select(&self, offered: &[u32]) -> Option<u32> {
        self.supported.iter().copied().find(|v| offered.contains(v))
    }

    /// Process a received Version Negotiation packet against the connection IDs
    /// the client used in its first Initial (`sent_scid` — the client's Source
    /// Connection ID, `sent_dcid` — the randomly chosen Destination Connection
    /// ID). On success the negotiator records that a Version Negotiation was
    /// processed and returns the [`VersionNegotiationOutcome`] to apply.
    ///
    /// # Errors
    ///
    /// [`VersionNegotiationError::AlreadyProcessed`] if one was already
    /// processed, [`VersionNegotiationError::NotVersionNegotiation`] if `packet`
    /// is not a Version Negotiation packet,
    /// [`VersionNegotiationError::ConnectionIdMismatch`] if the echoed
    /// connection IDs do not match, [`VersionNegotiationError::DowngradeDetected`]
    /// if the list includes the attempted version, or
    /// [`VersionNegotiationError::NoCompatibleVersion`] if no mutually supported
    /// version is offered. On any error the negotiator state is unchanged.
    pub fn process(
        &mut self,
        packet: &Packet,
        sent_scid: &[u8],
        sent_dcid: &[u8],
    ) -> Result<VersionNegotiationOutcome, VersionNegotiationError> {
        if self.processed {
            return Err(VersionNegotiationError::AlreadyProcessed);
        }
        let Packet::VersionNegotiation { dcid, scid, supported_versions, .. } = packet else {
            return Err(VersionNegotiationError::NotVersionNegotiation);
        };
        // RFC 9000 §6.1: the server echoes the client's Source CID as the
        // Destination CID and vice versa.
        if dcid.as_slice() != sent_scid || scid.as_slice() != sent_dcid {
            return Err(VersionNegotiationError::ConnectionIdMismatch);
        }
        // RFC 9000 §6.2: a Version Negotiation listing the attempted version is
        // forged or erroneous and MUST be discarded.
        if supported_versions.contains(&self.attempted) {
            return Err(VersionNegotiationError::DowngradeDetected);
        }
        let selected =
            self.select(supported_versions).ok_or(VersionNegotiationError::NoCompatibleVersion)?;
        self.processed = true;
        Ok(VersionNegotiationOutcome { selected_version: selected })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Version Negotiation packet echoing `dcid`/`scid` and offering
    /// `versions` (test helper).
    fn vn(dcid: &[u8], scid: &[u8], versions: Vec<u32>) -> Packet {
        Packet::VersionNegotiation {
            first_byte: 0x80,
            dcid: dcid.to_vec(),
            scid: scid.to_vec(),
            supported_versions: versions,
        }
    }

    /// The client's Source and Destination connection IDs for its first Initial.
    const CLIENT_SCID: &[u8] = &[0x11, 0x22, 0x33, 0x44];
    const CLIENT_DCID: &[u8] = &[0xaa, 0xbb, 0xcc, 0xdd, 0xee];

    #[test]
    fn constants_are_the_rfc_values() {
        assert_eq!(QUIC_VERSION_1, 1);
        assert_eq!(VERSION_NEGOTIATION_VERSION, 0);
    }

    #[test]
    fn default_supports_only_v1() {
        let neg = VersionNegotiator::new();
        assert_eq!(neg.attempted_version(), QUIC_VERSION_1);
        assert!(!neg.has_processed());
    }

    #[test]
    fn selects_a_supported_version() {
        // Client speaks v1 and a hypothetical v2, attempted v2; server offers v1.
        let mut neg = VersionNegotiator::with_supported(vec![0x0000_0002, QUIC_VERSION_1], 0x0000_0002);
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![QUIC_VERSION_1]);
        let outcome = neg.process(&packet, CLIENT_SCID, CLIENT_DCID).unwrap();
        assert_eq!(outcome.selected_version, QUIC_VERSION_1);
        assert!(neg.has_processed());
    }

    #[test]
    fn client_preference_order_wins() {
        // Client prefers v3 over v2; server offers both. Client picks v3.
        let mut neg =
            VersionNegotiator::with_supported(vec![0x0000_0003, 0x0000_0002], QUIC_VERSION_1);
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![0x0000_0002, 0x0000_0003]);
        let outcome = neg.process(&packet, CLIENT_SCID, CLIENT_DCID).unwrap();
        assert_eq!(outcome.selected_version, 0x0000_0003);
    }

    #[test]
    fn abandons_when_no_common_version() {
        // Client speaks only v1 (attempted a hypothetical v9); server offers v2/v3.
        let mut neg = VersionNegotiator::with_supported(vec![QUIC_VERSION_1], 0x0000_0009);
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![0x0000_0002, 0x0000_0003]);
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::NoCompatibleVersion)
        );
        // A failed process must not consume the one-packet budget.
        assert!(!neg.has_processed());
    }

    #[test]
    fn rejects_downgrade_listing_attempted_version() {
        // The server "offers" the very version the client attempted — forged.
        let mut neg = VersionNegotiator::new();
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![QUIC_VERSION_1, 0x0000_0002]);
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::DowngradeDetected)
        );
        assert!(!neg.has_processed());
    }

    #[test]
    fn rejects_wrong_dcid() {
        let mut neg = VersionNegotiator::with_supported(vec![0x0000_0002], QUIC_VERSION_1);
        let packet = vn(&[0x00, 0x00], CLIENT_DCID, vec![0x0000_0002]);
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::ConnectionIdMismatch)
        );
    }

    #[test]
    fn rejects_wrong_scid() {
        let mut neg = VersionNegotiator::with_supported(vec![0x0000_0002], QUIC_VERSION_1);
        let packet = vn(CLIENT_SCID, &[0x00, 0x00], vec![0x0000_0002]);
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::ConnectionIdMismatch)
        );
    }

    #[test]
    fn rejects_non_version_negotiation_packet() {
        let mut neg = VersionNegotiator::new();
        let packet = Packet::Short {
            spin: false,
            protected_bits: 0,
            dcid: CLIENT_SCID.to_vec(),
            protected: vec![0; 32],
        };
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::NotVersionNegotiation)
        );
    }

    #[test]
    fn rejects_second_version_negotiation() {
        let mut neg = VersionNegotiator::with_supported(vec![0x0000_0002], QUIC_VERSION_1);
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![0x0000_0002]);
        neg.process(&packet, CLIENT_SCID, CLIENT_DCID).unwrap();
        // A second Version Negotiation, even a valid-looking one, is discarded.
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::AlreadyProcessed)
        );
    }

    #[test]
    fn empty_offer_list_abandons() {
        let mut neg = VersionNegotiator::new();
        let packet = vn(CLIENT_SCID, CLIENT_DCID, Vec::new());
        assert_eq!(
            neg.process(&packet, CLIENT_SCID, CLIENT_DCID),
            Err(VersionNegotiationError::NoCompatibleVersion)
        );
    }

    #[test]
    fn ignores_unknown_grease_versions() {
        // A GREASE-like reserved version (0x?a?a?a?a) the client does not speak is
        // simply skipped; the client still selects its supported v2.
        let mut neg = VersionNegotiator::with_supported(vec![0x0000_0002], QUIC_VERSION_1);
        let packet = vn(CLIENT_SCID, CLIENT_DCID, vec![0x1a2a_3a4a, 0x0000_0002]);
        let outcome = neg.process(&packet, CLIENT_SCID, CLIENT_DCID).unwrap();
        assert_eq!(outcome.selected_version, 0x0000_0002);
    }
}
