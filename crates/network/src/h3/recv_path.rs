//! QUIC receive-path datagram ingest (RFC 9000 §8.1, §10.1, §12.2, §12.4;
//! RFC 9001 §5.5): the composition slice that drives the inbound decrypt path —
//! splitting one received UDP datagram into its coalesced QUIC packets,
//! decrypting each with the keys of its packet-number space, and feeding the
//! recovered frames into [`connection::QuicConnection::process_packet`](super::connection::QuicConnection::process_packet).
//!
//! This is the receive-path counterpart of the send-path
//! [`send_path::flush`](super::send_path::flush). Where [`flush`](super::send_path::flush)
//! folds each space's pending frames outward into coalesced datagrams and records
//! every sent packet into loss recovery, [`ingest_datagram`] draws one inbound
//! datagram inward: it walks the coalesced packets front to back (RFC 9000 §12.2),
//! decrypts each on a working copy through
//! [`packet_crypt::decrypt_packet`](super::packet_crypt::decrypt_packet), parses
//! its authenticated payload into frames, and routes them through the connection's
//! receive dispatch.
//!
//! ## Which packet gets which keys
//!
//! A datagram may coalesce packets from several encryption levels — a server's
//! first flight commonly carries an Initial and a Handshake packet back to back
//! (RFC 9000 §12.2). Each packet's long-/short-header form names its
//! [`PacketType`](super::packet_payload::PacketType), and thus its packet-number
//! space, before any decryption; [`ingest_datagram`] peeks the header
//! ([`packet::Packet::parse`](super::packet::Packet::parse)) to pick the right
//! [`SpaceKeys`] from the caller's [`RecvKeyRing`]. A packet whose space has no
//! keys installed yet (a 1-RTT packet that arrives before the handshake completes)
//! is left undecrypted and counted, not an error — the caller may buffer and retry
//! it once the keys exist, and because the header is length-delimited the walk can
//! still skip past it to the next coalesced packet.
//!
//! ## What is an error and what is a silent drop
//!
//! Only the content of an *authenticated* (successfully decrypted) packet can be a
//! connection error: a malformed frame is a `FRAME_ENCODING_ERROR` (RFC 9000
//! §12.4), a frame in a packet type that forbids it is a `PROTOCOL_VIOLATION`
//! (RFC 9000 §12.4, Table 3), and a frame that breaks a connection-level limit is
//! whatever [`connection::ProcessError`](super::connection::ProcessError) reports —
//! all surfaced as an [`IngestError`] the caller closes the connection with. An
//! unauthenticated packet, by contrast, is never trusted: a packet whose AEAD
//! authentication fails is silently discarded (RFC 9001 §5.5.2), and a truncated
//! or malformed coalesced header simply stops the walk (the remainder of the
//! datagram cannot be located), both merely counted in the [`IngestReport`].
//!
//! The module is pure apart from taking a caller-supplied `now`: the datagram
//! bytes come from [`event_loop::DatagramEventLoop::datagram`](super::event_loop::DatagramEventLoop::datagram),
//! and every key, packet number, and clock reading is supplied by the deterministic
//! lower slices. Deciding *when* to ingest and *when* to flush — the timer loop over
//! [`event_loop`](super::event_loop) tying this receive path to the
//! [`send_path`](super::send_path) — and the `h3_do_request` dispatch are the
//! connection driver's remaining job.

use std::time::Instant;

use super::connection::{PacketEffects, ProcessError, QuicConnection};
use super::key_schedule::PacketProtectionKeys;
use super::loss::PacketNumberSpace;
use super::packet::Packet;
use super::packet_crypt::decrypt_packet;
use super::packet_payload::PacketType;
use super::quic_frame::{self, QuicFrameError};

/// One packet-number space's receive-side packet-protection state: the keys that
/// open its packets and the largest packet number processed so far.
///
/// The keys are the peer's send-direction [`PacketProtectionKeys`] (what this
/// endpoint decrypts with). `largest_pn` seeds the truncated-packet-number
/// reconstruction (RFC 9000 §17.1, Appendix A.3); it starts at `0` and advances to
/// the highest number successfully decrypted in the space, so a later reordered
/// packet is still decoded relative to the highest seen.
#[derive(Clone, Debug)]
pub struct SpaceKeys {
    /// The packet-protection keys (peer's send direction) for this space.
    pub keys: PacketProtectionKeys,
    /// The largest packet number decrypted so far in this space, seeding
    /// truncated-number reconstruction (RFC 9000 §17.1, Appendix A.3).
    pub largest_pn: u64,
}

impl SpaceKeys {
    /// Wraps `keys` for a space that has processed no packets yet (`largest_pn`
    /// starts at `0`).
    pub fn new(keys: PacketProtectionKeys) -> Self {
        Self { keys, largest_pn: 0 }
    }
}

/// The receive keys for each packet-number space, installed as each encryption
/// level's keys become available during the handshake.
///
/// Initial keys exist from the first flight (RFC 9001 §5.2); Handshake and 1-RTT
/// keys are installed as the TLS handshake derives them. A space with no keys
/// installed cannot decrypt its packets yet, so [`ingest_datagram`] counts such a
/// packet as undecryptable rather than dropping it.
#[derive(Clone, Debug, Default)]
pub struct RecvKeyRing {
    /// Keys for the Initial packet-number space, if installed.
    initial: Option<SpaceKeys>,
    /// Keys for the Handshake packet-number space, if installed.
    handshake: Option<SpaceKeys>,
    /// Keys for the Application-Data (0-RTT / 1-RTT) packet-number space, if
    /// installed.
    application: Option<SpaceKeys>,
}

impl RecvKeyRing {
    /// An empty key ring with no space's keys installed yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs `keys` for `space`, resetting its `largest_pn` to `0`.
    ///
    /// Installing 1-RTT keys replaces any 0-RTT keys previously used for the same
    /// Application-Data space; a key update (RFC 9001 §6) is handled by
    /// [`crypto_state`](super::crypto_state), not by re-installing here.
    pub fn install(&mut self, space: PacketNumberSpace, keys: PacketProtectionKeys) {
        *self.slot_mut(space) = Some(SpaceKeys::new(keys));
    }

    /// A shared reference to `space`'s installed keys, or `None` if not installed.
    pub fn space(&self, space: PacketNumberSpace) -> Option<&SpaceKeys> {
        match space {
            PacketNumberSpace::Initial => self.initial.as_ref(),
            PacketNumberSpace::Handshake => self.handshake.as_ref(),
            PacketNumberSpace::ApplicationData => self.application.as_ref(),
        }
    }

    /// A mutable reference to `space`'s installed keys, or `None` if not installed.
    pub fn space_mut(&mut self, space: PacketNumberSpace) -> Option<&mut SpaceKeys> {
        self.slot_mut(space).as_mut()
    }

    /// The `Option` slot backing `space`.
    fn slot_mut(&mut self, space: PacketNumberSpace) -> &mut Option<SpaceKeys> {
        match space {
            PacketNumberSpace::Initial => &mut self.initial,
            PacketNumberSpace::Handshake => &mut self.handshake,
            PacketNumberSpace::ApplicationData => &mut self.application,
        }
    }
}

/// A summary of what one [`ingest_datagram`] did with a received datagram, plus the
/// merged [`PacketEffects`] the caller must action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// The number of coalesced packets successfully decrypted and dispatched.
    pub packets_processed: usize,
    /// The number of packets whose space had no keys installed yet: left
    /// undecrypted for the caller to buffer and retry once the keys exist.
    pub packets_undecryptable: usize,
    /// The number of authenticated-decryption failures (RFC 9001 §5.5.2) plus any
    /// remainder abandoned when a coalesced header could not be parsed: silently
    /// discarded.
    pub packets_dropped: usize,
    /// The number of Retry / Version Negotiation packets seen. These carry no
    /// frames and are handled by [`retry`](super::retry) /
    /// [`version_nego`](super::version_nego) before the handshake; the receive path
    /// only counts them.
    pub packets_non_frame_bearing: usize,
    /// The union of every processed packet's [`PacketEffects`]: the PATH_RESPONSE /
    /// RETIRE_CONNECTION_ID frames to send, the deferred frames (ACK, per-stream,
    /// NEW_TOKEN) to route, and the closing / handshake-confirmation flags.
    pub effects: PacketEffects,
}

/// A connection error raised by an *authenticated* packet's content, which the
/// caller closes the connection with using [`IngestError::code`].
///
/// Unauthenticated failures (AEAD authentication, a malformed coalesced header) are
/// never errors — they are silently discarded and only counted in the
/// [`IngestReport`] (RFC 9001 §5.5.2, RFC 9000 §12.2).
#[derive(Debug)]
pub enum IngestError {
    /// A decrypted payload held a malformed frame: `FRAME_ENCODING_ERROR`
    /// (RFC 9000 §12.4). Carries the underlying [`QuicFrameError`].
    Frame(QuicFrameError),
    /// A frame appeared in a packet type that forbids it: `PROTOCOL_VIOLATION`
    /// (RFC 9000 §12.4, Table 3).
    FrameNotPermitted {
        /// The offending frame's wire type code (RFC 9000 §19).
        frame_type: u64,
        /// The packet type it arrived in.
        packet_type: PacketType,
    },
    /// A frame broke a connection-level rule (flow control, the stream-count limit,
    /// the connection-ID rules, or the CRYPTO reassembly bound). Carries the
    /// underlying [`ProcessError`], whose [`ProcessError::code`] is the close code.
    Process(ProcessError),
}

impl IngestError {
    /// The QUIC transport error code to close the connection with (RFC 9000 §20.1).
    pub fn code(&self) -> u64 {
        match self {
            // FRAME_ENCODING_ERROR (RFC 9000 §20.1).
            Self::Frame(_) => 0x07,
            // PROTOCOL_VIOLATION (RFC 9000 §20.1).
            Self::FrameNotPermitted { .. } => 0x0a,
            Self::Process(e) => e.code(),
        }
    }
}

impl core::fmt::Display for IngestError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Frame(e) => write!(f, "QUIC receive path: malformed frame: {e}"),
            Self::FrameNotPermitted { frame_type, packet_type } => write!(
                f,
                "QUIC receive path: frame {frame_type:#x} not permitted in a {packet_type} packet"
            ),
            Self::Process(e) => write!(f, "QUIC receive path: {e}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(e) => Some(e),
            Self::Process(e) => Some(e),
            Self::FrameNotPermitted { .. } => None,
        }
    }
}

impl From<QuicFrameError> for IngestError {
    fn from(e: QuicFrameError) -> Self {
        Self::Frame(e)
    }
}

impl From<ProcessError> for IngestError {
    fn from(e: ProcessError) -> Self {
        Self::Process(e)
    }
}

/// The [`PacketType`] of a parsed packet header, or `None` for the non-frame-bearing
/// Retry / Version Negotiation forms (RFC 9000 §17.2.1, §17.2.5).
fn frame_bearing_type(header: &Packet) -> Option<PacketType> {
    match header {
        Packet::Initial { .. } => Some(PacketType::Initial),
        Packet::ZeroRtt { .. } => Some(PacketType::ZeroRtt),
        Packet::Handshake { .. } => Some(PacketType::Handshake),
        Packet::Short { .. } => Some(PacketType::OneRtt),
        Packet::Retry { .. } | Packet::VersionNegotiation { .. } => None,
    }
}

/// Merge one processed packet's [`PacketEffects`] into the running report.
fn merge_effects(into: &mut PacketEffects, from: PacketEffects) {
    into.responses.extend(from.responses);
    into.retire_connection_ids.extend(from.retire_connection_ids);
    into.deferred.extend(from.deferred);
    into.peer_closed |= from.peer_closed;
    into.handshake_confirmed |= from.handshake_confirmed;
    into.ack_eliciting |= from.ack_eliciting;
}

/// Ingest one received UDP datagram into `conn`, decrypting and dispatching each of
/// its coalesced packets.
///
/// The datagram's byte count is first credited to the connection's
/// anti-amplification limit and its idle timer is restarted
/// ([`QuicConnection::on_datagram_received`](super::connection::QuicConnection::on_datagram_received),
/// RFC 9000 §8.1, §10.1). Then each coalesced packet (RFC 9000 §12.2) is walked
/// front to back:
///
/// - a Retry / Version Negotiation packet is counted (it carries no frames and is
///   handled elsewhere) and, consuming the rest of the datagram, ends the walk;
/// - a frame-bearing packet whose space has no keys in `keys` is counted as
///   undecryptable and skipped;
/// - otherwise the packet is decrypted; an AEAD failure discards it silently
///   (RFC 9001 §5.5.2), and a success parses the payload into frames, checks each
///   against the packet type's permission table (RFC 9000 §12.4), and dispatches
///   them through [`QuicConnection::process_packet`](super::connection::QuicConnection::process_packet),
///   advancing the space's `largest_pn`.
///
/// `local_cid_len` is the length of the connection IDs this endpoint issued (it
/// delimits a short-header Destination Connection ID, RFC 9000 §17.3.1).
///
/// # Errors
///
/// [`IngestError`] when an *authenticated* packet's content is a connection error:
/// a malformed frame ([`IngestError::Frame`]), a frame barred from its packet type
/// ([`IngestError::FrameNotPermitted`]), or a connection-level violation
/// ([`IngestError::Process`]). Unauthenticated failures never error; they are
/// counted in the returned [`IngestReport`].
pub fn ingest_datagram(
    conn: &mut QuicConnection,
    keys: &mut RecvKeyRing,
    datagram: &[u8],
    local_cid_len: usize,
    now: Instant,
) -> Result<IngestReport, IngestError> {
    conn.on_datagram_received(datagram.len() as u64, now);

    let mut report = IngestReport::default();
    let mut offset = 0usize;

    while offset < datagram.len() {
        let rest = &datagram[offset..];

        // Peek the header to learn the packet type (and thus space) before choosing
        // keys. A malformed header cannot be located past, so the walk stops and the
        // remainder is discarded (RFC 9000 §12.2).
        let (header, peeked) = match Packet::parse(rest, local_cid_len) {
            Ok(parsed) => parsed,
            Err(_) => {
                report.packets_dropped += 1;
                break;
            }
        };

        let Some(packet_type) = frame_bearing_type(&header) else {
            // Retry / Version Negotiation: no frames, handled by other slices, and
            // consumes the rest of the datagram.
            report.packets_non_frame_bearing += 1;
            offset += peeked;
            continue;
        };
        let space = packet_type.number_space();

        let Some(space_keys) = keys.space_mut(space) else {
            // No keys for this space yet (e.g. a 1-RTT packet before the handshake
            // completes): leave it for the caller to buffer and retry.
            report.packets_undecryptable += 1;
            offset += peeked;
            continue;
        };

        let decrypted =
            match decrypt_packet(&space_keys.keys, rest, local_cid_len, space_keys.largest_pn) {
                Ok(decrypted) => decrypted,
                Err(_) => {
                    // Authentication failed: discard silently (RFC 9001 §5.5.2) and
                    // move on using the in-the-clear header length.
                    report.packets_dropped += 1;
                    offset += peeked;
                    continue;
                }
            };

        // A malformed frame in an authenticated packet is FRAME_ENCODING_ERROR.
        let frames = quic_frame::parse_all(&decrypted.payload)?;

        // A frame in a packet type that forbids it is PROTOCOL_VIOLATION
        // (RFC 9000 §12.4, Table 3).
        for frame in &frames {
            if !packet_type.permits(frame) {
                return Err(IngestError::FrameNotPermitted {
                    frame_type: frame.frame_type(),
                    packet_type,
                });
            }
        }

        let effects = conn.process_packet(space, decrypted.packet_number, &frames, now)?;
        merge_effects(&mut report.effects, effects);

        // Advance the space's largest processed number for later truncated-number
        // reconstruction, and step to the next coalesced packet.
        if let Some(space_keys) = keys.space_mut(space) {
            space_keys.largest_pn = space_keys.largest_pn.max(decrypted.packet_number);
        }
        report.packets_processed += 1;
        offset += decrypted.consumed;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::connection::ConnectionConfig;
    use crate::h3::key_schedule::InitialKeys;
    use crate::h3::packet_crypt::{ProtectedHeader, encrypt_packet};
    use crate::h3::quic_frame::Frame;
    use std::time::Duration;

    /// A fixed instant; the module never reads the clock itself.
    fn now() -> Instant {
        Instant::now()
    }

    /// The RFC 9001 Appendix A client Destination Connection ID.
    fn dcid() -> Vec<u8> {
        vec![0x83, 0x94, 0xc8, 0xf0, 0x3e, 0x51, 0x57, 0x08]
    }

    fn keys() -> InitialKeys {
        InitialKeys::derive(&dcid())
    }

    /// A client connection whose peer advertised generous limits, so a stray
    /// MAX_DATA / MAX_STREAMS never trips a limit during dispatch.
    fn connection() -> QuicConnection {
        QuicConnection::new_client(
            ConnectionConfig {
                peer_initial_cid: dcid(),
                local_initial_cid: vec![0x11, 0x22, 0x33, 0x44],
                active_connection_id_limit: 8,
                peer_active_connection_id_limit: 8,
                peer_initial_max_data: 1_000_000,
                peer_initial_max_streams_bidi: 100,
                peer_initial_max_streams_uni: 100,
                pto: Duration::from_millis(100),
            },
            now(),
        )
    }

    /// A key ring with the Initial keys installed for the Initial space.
    fn initial_ring() -> RecvKeyRing {
        let mut ring = RecvKeyRing::new();
        ring.install(PacketNumberSpace::Initial, keys().client);
        ring
    }

    fn crypto(offset: u64, len: usize) -> Frame {
        Frame::Crypto { offset, data: vec![0xAB; len] }
    }

    /// Encrypt one Initial packet carrying `frames` with packet number `pn`.
    fn initial_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Initial { version: 1, dcid: &dcid, scid: &[], token: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    /// Encrypt one Handshake packet carrying `frames` with packet number `pn`.
    fn handshake_packet(pn: u64, frames: &[Frame]) -> Vec<u8> {
        let dcid = dcid();
        let header = ProtectedHeader::Handshake { version: 1, dcid: &dcid, scid: &[] };
        let mut payload = Vec::new();
        quic_frame::encode_all(frames, &mut payload).expect("encode frames");
        encrypt_packet(&keys().client, &header, pn, None, &payload).expect("encrypt")
    }

    // ---- the happy path -------------------------------------------------

    #[test]
    fn ingest_empty_datagram_processes_nothing() {
        let mut conn = connection();
        let mut ring = initial_ring();
        let report = ingest_datagram(&mut conn, &mut ring, &[], 4, now()).unwrap();
        assert_eq!(report, IngestReport::default());
    }

    #[test]
    fn ingest_one_initial_packet_dispatches_its_frames() {
        let mut conn = connection();
        let mut ring = initial_ring();
        let dg = initial_packet(0, &[crypto(0, 16)]);

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 1);
        assert_eq!(report.packets_dropped, 0);
        assert_eq!(report.packets_undecryptable, 0);
        // CRYPTO is ack-eliciting.
        assert!(report.effects.ack_eliciting);
        // The CRYPTO frame reassembled into the Initial space.
        assert_eq!(conn.read_crypto(PacketNumberSpace::Initial).len(), 16);
    }

    #[test]
    fn ingest_advances_largest_pn() {
        let mut conn = connection();
        let mut ring = initial_ring();

        // PADDING pads the packet so header protection has its 16-byte sample.
        let dg = initial_packet(3, &[Frame::Ping, Frame::Padding(24)]);
        ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(
            ring.space(PacketNumberSpace::Initial).unwrap().largest_pn,
            3,
            "largest_pn tracks the highest decrypted number"
        );
    }

    #[test]
    fn ingest_credits_anti_amplification_and_restarts_idle_timer() {
        let mut conn = connection();
        let mut ring = initial_ring();
        let dg = initial_packet(0, &[crypto(0, 16)]);
        let len = dg.len();

        // The received bytes lift the 3x anti-amplification allowance
        // (RFC 9000 §8.1): after receiving `len` bytes (and sending nothing) the
        // connection may send 3*len before validation.
        ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(
            conn.anti_amplification().send_allowance(),
            Some((len as u64) * 3),
            "receiving {len} bytes should credit the anti-amplification budget"
        );
    }

    // ---- coalescing -----------------------------------------------------

    #[test]
    fn ingest_walks_coalesced_initial_and_handshake() {
        let mut conn = connection();
        let mut ring = RecvKeyRing::new();
        ring.install(PacketNumberSpace::Initial, keys().client);
        ring.install(PacketNumberSpace::Handshake, keys().client);

        let mut dg = initial_packet(0, &[crypto(0, 8)]);
        dg.extend(handshake_packet(0, &[crypto(0, 8)]));

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 2, "both coalesced packets dispatched");
        assert_eq!(conn.read_crypto(PacketNumberSpace::Initial).len(), 8);
        assert_eq!(conn.read_crypto(PacketNumberSpace::Handshake).len(), 8);
    }

    #[test]
    fn ingest_counts_a_packet_with_no_keys_as_undecryptable() {
        let mut conn = connection();
        // Only Initial keys installed; the coalesced Handshake packet cannot be
        // decrypted and is left for later.
        let mut ring = initial_ring();

        let mut dg = initial_packet(0, &[crypto(0, 8)]);
        dg.extend(handshake_packet(0, &[crypto(0, 8)]));

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 1, "Initial dispatched");
        assert_eq!(report.packets_undecryptable, 1, "Handshake left undecrypted");
        assert_eq!(conn.read_crypto(PacketNumberSpace::Handshake).len(), 0);
    }

    // ---- effects merging ------------------------------------------------

    #[test]
    fn ingest_defers_an_ack_frame_to_the_report() {
        let mut conn = connection();
        let mut ring = initial_ring();
        // An ACK is permitted in an Initial packet and is deferred to loss detection
        // (a later slice), surfacing in the merged effects.
        let dg = initial_packet(
            0,
            &[Frame::Ack {
                largest_acked: 0,
                ack_delay: 0,
                first_ack_range: 0,
                ranges: vec![],
                ecn: None,
            }],
        );

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 1);
        // ACK is deferred to loss detection (a later slice).
        assert_eq!(report.effects.deferred.len(), 1);
        assert!(matches!(report.effects.deferred[0], Frame::Ack { .. }));
        // A pure-ACK packet is not ack-eliciting.
        assert!(!report.effects.ack_eliciting);
    }

    // ---- silent drops ---------------------------------------------------

    #[test]
    fn ingest_drops_a_packet_that_fails_authentication() {
        let mut conn = connection();
        let mut ring = initial_ring();
        let mut dg = initial_packet(0, &[crypto(0, 16)]);
        // Corrupt the AEAD tag / ciphertext tail so authentication fails.
        let last = dg.len() - 1;
        dg[last] ^= 0xFF;

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 0);
        assert_eq!(report.packets_dropped, 1);
        assert_eq!(conn.read_crypto(PacketNumberSpace::Initial).len(), 0);
    }

    #[test]
    fn ingest_stops_and_counts_on_a_malformed_trailing_header() {
        let mut conn = connection();
        let mut ring = initial_ring();
        let mut dg = initial_packet(0, &[crypto(0, 8)]);
        // Append a truncated long-header byte that cannot parse as a full packet.
        dg.push(0xC0);

        let report = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap();
        assert_eq!(report.packets_processed, 1, "the valid Initial still dispatched");
        assert_eq!(report.packets_dropped, 1, "the truncated remainder is discarded");
    }

    // ---- authenticated connection errors --------------------------------

    #[test]
    fn ingest_rejects_a_frame_barred_from_the_packet_type() {
        let mut conn = connection();
        let mut ring = initial_ring();
        // HANDSHAKE_DONE is 1-RTT only (RFC 9000 §12.4); in an Initial it is a
        // PROTOCOL_VIOLATION. PADDING gives header protection its 16-byte sample.
        let dg = initial_packet(0, &[Frame::HandshakeDone, Frame::Padding(24)]);

        let err = ingest_datagram(&mut conn, &mut ring, &dg, 4, now()).unwrap_err();
        match err {
            IngestError::FrameNotPermitted { packet_type, .. } => {
                assert_eq!(packet_type, PacketType::Initial);
            }
            other => panic!("expected FrameNotPermitted, got {other:?}"),
        }
        // PROTOCOL_VIOLATION close code.
        assert_eq!(err.code(), 0x0a);
    }

    #[test]
    fn ingest_surfaces_a_connection_level_violation() {
        // Retiring a connection-ID sequence higher than any we issued (we issued
        // only seq 0) is a PROTOCOL_VIOLATION the dispatch surfaces (RFC 9000
        // §19.16). RETIRE_CONNECTION_ID is application-only, so carry it in a 1-RTT
        // packet.
        let mut conn = connection();
        let mut ring = RecvKeyRing::new();
        ring.install(PacketNumberSpace::ApplicationData, keys().client);

        let dcid = dcid();
        let header = ProtectedHeader::Short { spin: false, key_phase: false, dcid: &dcid };
        let mut payload = Vec::new();
        // PADDING gives header protection its 16-byte sample.
        quic_frame::encode_all(&[Frame::RetireConnectionId(5), Frame::Padding(24)], &mut payload)
            .unwrap();
        let dg = encrypt_packet(&keys().client, &header, 0, None, &payload).unwrap();

        let err = ingest_datagram(&mut conn, &mut ring, &dg, dcid.len(), now()).unwrap_err();
        assert!(matches!(err, IngestError::Process(_)), "{err:?}");
    }

    // ---- key ring -------------------------------------------------------

    #[test]
    fn key_ring_install_and_lookup() {
        let mut ring = RecvKeyRing::new();
        assert!(ring.space(PacketNumberSpace::Initial).is_none());
        ring.install(PacketNumberSpace::Initial, keys().client);
        assert!(ring.space(PacketNumberSpace::Initial).is_some());
        assert_eq!(ring.space(PacketNumberSpace::Initial).unwrap().largest_pn, 0);
        // Other spaces remain empty.
        assert!(ring.space(PacketNumberSpace::Handshake).is_none());
        assert!(ring.space(PacketNumberSpace::ApplicationData).is_none());
    }
}
