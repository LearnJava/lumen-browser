//! QUIC connection-level receive dispatch (RFC 9000 §12.4, §13, §19): the first
//! composition slice of the connection engine — the single place that owns the
//! connection-wide state machines and routes each decrypted QUIC frame to the
//! machine that owns it.
//!
//! Every earlier slice is a self-contained state machine or codec; none of them
//! knows about the others. This slice is the first that *composes* them.
//! [`QuicConnection`] holds the connection-wide receiver state — the per-space
//! acknowledgement generators ([`ack::AckGenerator`]), the CRYPTO reassembly
//! buffers ([`crypto_stream::CryptoRecvStream`]), our view of the peer's
//! connection flow-control and stream-count limits ([`conn_flow`]), the peer's
//! and our connection-ID sets ([`conn_id`]), the path validator and
//! anti-amplification limit ([`path_validation`]), and the connection lifecycle
//! ([`lifecycle`]) — and drives them from one decrypted packet at a time.
//!
//! ## What this slice owns and what it defers
//!
//! [`QuicConnection::process_packet`] takes a packet's decoded frames (the output
//! of [`packet_crypt::decrypt_packet`](super::packet_crypt::decrypt_packet) then
//! [`quic_frame::parse_all`](super::quic_frame::parse_all)) and dispatches each:
//!
//! - Connection-wide control frames are handled in full here: MAX_DATA and
//!   MAX_STREAMS raise our send limits, NEW_CONNECTION_ID / RETIRE_CONNECTION_ID
//!   drive the connection-ID sets, PATH_CHALLENGE is echoed and PATH_RESPONSE
//!   drives path validation, CRYPTO is reassembled per space, and
//!   CONNECTION_CLOSE / HANDSHAKE_DONE move the lifecycle.
//! - Frames owned by machinery this slice does not hold are surfaced in
//!   [`PacketEffects::deferred`] for a later slice to route: ACK (loss detection,
//!   [`super::pto`]), the per-stream frames (the stream manager), and NEW_TOKEN.
//!
//! The module is pure and clock-driven by a caller-supplied `now`, exactly like
//! the machines it composes, so it is driven deterministically in tests with no
//! socket. Packet decryption, packet-number decoding, and the send path are the
//! caller's job; wiring this dispatcher under [`super::event_loop`] is the next
//! slice.

use std::time::{Duration, Instant};

use super::ack::{AckGenerator, AckUrgency, EcnCodepoint};
use super::conn_flow::{ConnError, SendConnFlow, SendStreamLimit, StreamDir};
use super::conn_id::{ConnIdError, LocalConnIds, RemoteConnIds};
use super::crypto_stream::{CryptoRecvStream, CryptoStreamError};
use super::lifecycle::ConnectionLifecycle;
use super::loss::PacketNumberSpace;
use super::path_validation::{AntiAmplificationLimit, PathValidator, respond_to_challenge};
use super::quic_frame::Frame;
use super::timer::ConnectionTimers;

/// The default `max_ack_delay` (RFC 9000 §18.2) the Application Data space uses
/// when delaying acknowledgements: 25 milliseconds.
pub const DEFAULT_MAX_ACK_DELAY: Duration = Duration::from_millis(25);

/// A connection-level protocol violation surfaced while dispatching a packet's
/// frames. Each variant wraps the owning state machine's error and forwards its
/// RFC 9000 §20.1 connection-error [`ProcessError::code`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    /// A flow-control or stream-count limit was violated (from [`conn_flow`]).
    Flow(ConnError),
    /// A connection-ID frame was malformed or violated the ID rules (from
    /// [`conn_id`]).
    ConnId(ConnIdError),
    /// The peer sent CRYPTO data past the reassembly bound (from
    /// [`crypto_stream`]).
    Crypto(CryptoStreamError),
}

impl ProcessError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::Flow(e) => e.code(),
            Self::ConnId(e) => e.code(),
            Self::Crypto(e) => e.code(),
        }
    }
}

impl core::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Flow(e) => write!(f, "{e}"),
            Self::ConnId(e) => write!(f, "{e}"),
            Self::Crypto(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<ConnError> for ProcessError {
    fn from(e: ConnError) -> Self {
        Self::Flow(e)
    }
}

impl From<ConnIdError> for ProcessError {
    fn from(e: ConnIdError) -> Self {
        Self::ConnId(e)
    }
}

impl From<CryptoStreamError> for ProcessError {
    fn from(e: CryptoStreamError) -> Self {
        Self::Crypto(e)
    }
}

/// The configuration a [`QuicConnection`] needs at construction: the connection
/// IDs exchanged during the handshake and the transport-parameter limits the
/// peer advertised (RFC 9000 §18.2), which seed our send-side limits.
#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    /// The peer's Source Connection ID from its first long-header packet: the ID
    /// we stamp on packets we send, seeding [`RemoteConnIds`] at sequence 0.
    pub peer_initial_cid: Vec<u8>,
    /// The connection ID we chose for the peer to route packets back to us,
    /// seeding [`LocalConnIds`] at sequence 0.
    pub local_initial_cid: Vec<u8>,
    /// The `active_connection_id_limit` we advertised: the most connection IDs
    /// the peer may leave us holding (RFC 9000 §5.1.1).
    pub active_connection_id_limit: u64,
    /// The peer's `active_connection_id_limit`: how many IDs it will accept from
    /// us before we must retire one.
    pub peer_active_connection_id_limit: u64,
    /// The peer's `initial_max_data`: the connection-wide byte budget our send
    /// side starts with (RFC 9000 §4.1).
    pub peer_initial_max_data: u64,
    /// The peer's `initial_max_streams_bidi`: how many bidirectional streams we
    /// may open initially (RFC 9000 §4.6).
    pub peer_initial_max_streams_bidi: u64,
    /// The peer's `initial_max_streams_uni`: how many unidirectional streams we
    /// may open initially (RFC 9000 §4.6).
    pub peer_initial_max_streams_uni: u64,
    /// The probe timeout used for the idle-timeout floor and the closing /
    /// draining period (RFC 9000 §10). Refreshed as the RTT estimate evolves via
    /// [`QuicConnection::set_pto`].
    pub pto: Duration,
}

/// What the caller must do after [`QuicConnection::process_packet`] dispatched a
/// packet's frames: the frames to send in response, the connection IDs to retire,
/// and the frames deferred to subsystems this slice does not own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PacketEffects {
    /// PATH_RESPONSE frames to send, one per PATH_CHALLENGE received
    /// (RFC 9000 §8.2.2).
    pub responses: Vec<Frame>,
    /// Sequence numbers of connection IDs the peer's NEW_CONNECTION_ID frames
    /// forced us to retire; the caller sends a RETIRE_CONNECTION_ID for each
    /// (RFC 9000 §19.15).
    pub retire_connection_ids: Vec<u64>,
    /// Frames owned by machinery this slice does not hold — ACK (loss detection),
    /// the per-stream frames (the stream manager), and NEW_TOKEN — for a later
    /// slice to route. Preserved in arrival order.
    pub deferred: Vec<Frame>,
    /// Whether the peer sent CONNECTION_CLOSE: the connection has entered the
    /// draining state and sends nothing further (RFC 9000 §10.2.2).
    pub peer_closed: bool,
    /// Whether the peer sent HANDSHAKE_DONE, confirming the handshake
    /// (RFC 9000 §19.20).
    pub handshake_confirmed: bool,
    /// Whether the packet was ack-eliciting (carried any frame other than ACK,
    /// PADDING, or CONNECTION_CLOSE, RFC 9002 §2): the caller consults the space's
    /// [`AckGenerator`] for the resulting acknowledgement obligation.
    pub ack_eliciting: bool,
}

/// The connection-wide receiver state of a QUIC client connection, composing
/// every earlier slice's state machine (RFC 9000 §12.4, §13, §19).
///
/// Owns the three per-space acknowledgement generators and CRYPTO reassembly
/// buffers, the send-side connection flow-control and stream-count limits, the
/// peer's and our connection-ID sets, the path validator and anti-amplification
/// limit, and the connection lifecycle. Frames are fed one packet at a time
/// through [`QuicConnection::process_packet`]; the caller decrypts and decodes
/// the packet first.
#[derive(Debug)]
pub struct QuicConnection {
    /// Acknowledgement generator for the Initial packet-number space.
    ack_initial: AckGenerator,
    /// Acknowledgement generator for the Handshake packet-number space.
    ack_handshake: AckGenerator,
    /// Acknowledgement generator for the Application Data packet-number space.
    ack_app: AckGenerator,
    /// CRYPTO reassembly for the Initial encryption level.
    crypto_initial: CryptoRecvStream,
    /// CRYPTO reassembly for the Handshake encryption level.
    crypto_handshake: CryptoRecvStream,
    /// CRYPTO reassembly for the 1-RTT (Application Data) encryption level.
    crypto_app: CryptoRecvStream,
    /// Our send-side view of the peer's connection-wide flow-control limit.
    send_flow: SendConnFlow,
    /// Our send-side view of the peer's bidirectional stream-count limit.
    send_bidi_limit: SendStreamLimit,
    /// Our send-side view of the peer's unidirectional stream-count limit.
    send_uni_limit: SendStreamLimit,
    /// The connection IDs the peer issued for us to send to.
    remote_cids: RemoteConnIds,
    /// The connection IDs we issued for the peer to send to.
    local_cids: LocalConnIds,
    /// The path validator driving PATH_CHALLENGE / PATH_RESPONSE.
    path: PathValidator,
    /// The anti-amplification limit on the peer's (initially unvalidated) address.
    anti_amplification: AntiAmplificationLimit,
    /// The connection lifecycle (active / closing / draining / closed).
    lifecycle: ConnectionLifecycle,
    /// Whether the peer has confirmed the handshake (HANDSHAKE_DONE received).
    handshake_confirmed: bool,
    /// The probe timeout feeding the idle-timeout floor and closing period.
    pto: Duration,
}

impl QuicConnection {
    /// Builds a client connection's receiver state from the handshake-exchanged
    /// connection IDs and the peer's advertised transport-parameter limits.
    ///
    /// `now` seeds the lifecycle's idle-timeout baseline.
    pub fn new_client(config: ConnectionConfig, now: Instant) -> Self {
        Self {
            ack_initial: AckGenerator::new(PacketNumberSpace::Initial, DEFAULT_MAX_ACK_DELAY),
            ack_handshake: AckGenerator::new(PacketNumberSpace::Handshake, DEFAULT_MAX_ACK_DELAY),
            ack_app: AckGenerator::new(PacketNumberSpace::ApplicationData, DEFAULT_MAX_ACK_DELAY),
            crypto_initial: CryptoRecvStream::new(),
            crypto_handshake: CryptoRecvStream::new(),
            crypto_app: CryptoRecvStream::new(),
            send_flow: SendConnFlow::new(config.peer_initial_max_data),
            send_bidi_limit: SendStreamLimit::new(
                StreamDir::Bidi,
                config.peer_initial_max_streams_bidi,
            ),
            send_uni_limit: SendStreamLimit::new(
                StreamDir::Uni,
                config.peer_initial_max_streams_uni,
            ),
            remote_cids: RemoteConnIds::new(
                config.peer_initial_cid,
                config.active_connection_id_limit,
            ),
            local_cids: LocalConnIds::new(
                config.local_initial_cid,
                config.peer_active_connection_id_limit,
            ),
            path: PathValidator::new(),
            anti_amplification: AntiAmplificationLimit::new(),
            lifecycle: ConnectionLifecycle::new(now),
            handshake_confirmed: false,
            pto: config.pto,
        }
    }

    /// Updates the probe timeout used for the idle-timeout floor and the closing /
    /// draining period as the RTT estimate evolves (RFC 9000 §10).
    pub fn set_pto(&mut self, pto: Duration) {
        self.pto = pto;
    }

    /// Sets the effective idle timeout negotiated from both endpoints'
    /// `max_idle_timeout` transport parameters (RFC 9000 §10.1); `None` disables
    /// the idle timer. Compute it with
    /// [`effective_idle_timeout`](super::lifecycle::effective_idle_timeout).
    pub fn set_idle_timeout(&mut self, timeout: Option<Duration>) {
        self.lifecycle.set_idle_timeout(timeout);
    }

    /// Records that a datagram of `datagram_len` bytes arrived at `now`, feeding
    /// the anti-amplification credit (RFC 9000 §8.1) and restarting the idle
    /// timer (RFC 9000 §10.1). Call this once per received datagram, before
    /// [`QuicConnection::process_packet`] for the packets it carried.
    pub fn on_datagram_received(&mut self, datagram_len: u64, now: Instant) {
        self.anti_amplification.on_received(datagram_len);
        self.lifecycle.on_packet_received(now);
    }

    /// Dispatches every frame of one decrypted packet, driving the owning state
    /// machine for each and returning the [`PacketEffects`] the caller must action.
    ///
    /// `space` is the packet's number space, `packet_number` its decoded number,
    /// and `frames` its decoded payload. The packet number is recorded for the
    /// space's acknowledgement generator; each frame is then routed. A frame that
    /// violates a connection-level limit stops dispatch with a [`ProcessError`]
    /// (the caller closes the connection with [`ProcessError::code`]); frames
    /// already dispatched before the error keep their effect, as the connection is
    /// about to close regardless.
    ///
    /// # Errors
    ///
    /// [`ProcessError`] when a frame violates flow control, the stream-count
    /// limit, the connection-ID rules, or the CRYPTO reassembly bound.
    pub fn process_packet(
        &mut self,
        space: PacketNumberSpace,
        packet_number: u64,
        frames: &[Frame],
        now: Instant,
    ) -> Result<PacketEffects, ProcessError> {
        let ack_eliciting = frames.iter().any(is_ack_eliciting);

        self.ack_generator_mut(space).on_packet_received(
            packet_number,
            ack_eliciting,
            EcnCodepoint::NotEct,
            now,
        );

        let mut effects = PacketEffects {
            ack_eliciting,
            ..PacketEffects::default()
        };

        for frame in frames {
            self.dispatch_frame(space, frame, now, &mut effects)?;
        }

        Ok(effects)
    }

    /// Routes one frame to its owning state machine, recording any effect.
    fn dispatch_frame(
        &mut self,
        space: PacketNumberSpace,
        frame: &Frame,
        now: Instant,
        effects: &mut PacketEffects,
    ) -> Result<(), ProcessError> {
        match frame {
            // No connection-wide state; PING's ack-eliciting nature is already
            // captured by `ack_eliciting`.
            Frame::Padding(_) | Frame::Ping => {}

            // Loss detection owns ACK processing; not held by this slice.
            Frame::Ack { .. } => effects.deferred.push(frame.clone()),

            // The stream manager (a later slice) owns per-stream state.
            Frame::ResetStream { .. }
            | Frame::StopSending { .. }
            | Frame::Stream { .. }
            | Frame::MaxStreamData { .. }
            | Frame::StreamDataBlocked { .. }
            | Frame::NewToken(_) => effects.deferred.push(frame.clone()),

            Frame::Crypto { offset, data } => {
                self.crypto_recv_mut(space).recv(*offset, data)?;
            }

            // MAX_DATA raises our connection-wide send budget (RFC 9000 §19.9).
            Frame::MaxData(max) => self.send_flow.update_max_data(*max),

            // MAX_STREAMS raises how many streams we may open (RFC 9000 §19.11).
            Frame::MaxStreams { bidi, max } => {
                if *bidi {
                    self.send_bidi_limit.update_max_streams(*max);
                } else {
                    self.send_uni_limit.update_max_streams(*max);
                }
            }

            // DATA_BLOCKED / STREAMS_BLOCKED are the peer signalling it wants a
            // higher limit; the receive-side window updates that answer them are
            // driven by the stream manager as data is consumed, so there is no
            // connection-wide state to move here (RFC 9000 §19.12, §19.14).
            Frame::DataBlocked(_) | Frame::StreamsBlocked { .. } => {}

            Frame::NewConnectionId {
                sequence_number,
                retire_prior_to,
                connection_id,
                stateless_reset_token,
            } => {
                let retired = self.remote_cids.record_new_connection_id(
                    *sequence_number,
                    *retire_prior_to,
                    connection_id.clone(),
                    *stateless_reset_token,
                )?;
                effects.retire_connection_ids.extend(retired);
            }

            // The peer retires one of the IDs we issued (RFC 9000 §19.16).
            Frame::RetireConnectionId(seq) => self.local_cids.retire(*seq)?,

            // Echo a path challenge back to the peer (RFC 9000 §8.2.2).
            Frame::PathChallenge(data) => effects.responses.push(respond_to_challenge(*data)),

            // A response to our challenge validates the path (RFC 9000 §8.2.3),
            // which also lifts the anti-amplification limit (RFC 9000 §8.1).
            Frame::PathResponse(data) => {
                if self.path.on_path_response(*data) {
                    self.anti_amplification.mark_validated();
                }
            }

            // The peer closes the connection; we enter the draining state
            // (RFC 9000 §10.2.2).
            Frame::ConnectionClose { .. } => {
                self.lifecycle.on_connection_close_received(now, self.pto);
                effects.peer_closed = true;
            }

            // The server confirms the handshake (RFC 9000 §19.20), which also
            // validates our path to it (RFC 9000 §8.1).
            Frame::HandshakeDone => {
                self.handshake_confirmed = true;
                self.anti_amplification.mark_validated();
                effects.handshake_confirmed = true;
            }
        }
        Ok(())
    }

    /// Begins validating the current path by recording and returning a
    /// `PATH_CHALLENGE` carrying `data` (eight caller-chosen unpredictable bytes),
    /// arming the abandon timer at `now + 3·PTO` (RFC 9000 §8.2.1, §8.2.4). A
    /// matching PATH_RESPONSE later validates the path and lifts the
    /// anti-amplification limit. Returns `None` once validation has completed.
    pub fn send_path_challenge(&mut self, data: [u8; 8], now: Instant) -> Option<Frame> {
        self.path.send_challenge(data, now, self.pto)
    }

    /// Folds every owned machine's current deadline into `timers`, so the event
    /// loop can arm a single OS timer for the earliest (RFC 9000 §8.2.4, §10.1,
    /// §13.2.1). The loss-detection / PTO timer is owned by the send engine (a
    /// later slice) and left untouched here.
    pub fn refresh_timers(&self, timers: &mut ConnectionTimers) {
        timers.set_idle_timeout(self.lifecycle.idle_deadline(self.pto));
        timers.set_draining_close(self.lifecycle.close_deadline());
        timers.set_path_validation(self.path.deadline());
        timers.set_ack_delay(PacketNumberSpace::Initial, self.ack_initial.ack_urgency());
        timers.set_ack_delay(
            PacketNumberSpace::Handshake,
            self.ack_handshake.ack_urgency(),
        );
        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            self.ack_app.ack_urgency(),
        );
    }

    /// Builds the pending acknowledgement for `space` at `now`, if any is owed,
    /// clearing that space's pending-ack state (RFC 9000 §19.3). `ack_delay_exponent`
    /// is the peer's advertised value (RFC 9000 §18.2).
    pub fn generate_ack(
        &mut self,
        space: PacketNumberSpace,
        now: Instant,
        ack_delay_exponent: u64,
    ) -> Option<Frame> {
        self.ack_generator_mut(space)
            .generate_ack_frame(now, ack_delay_exponent)
    }

    /// The acknowledgement urgency currently owed for `space` (RFC 9000 §13.2.1).
    pub fn ack_urgency(&self, space: PacketNumberSpace) -> AckUrgency {
        self.ack_generator(space).ack_urgency()
    }

    /// Reads the contiguous reassembled CRYPTO prefix available at `space`'s
    /// encryption level, advancing the read cursor (RFC 9000 §7.5). Empty when no
    /// new contiguous handshake bytes are ready.
    pub fn read_crypto(&mut self, space: PacketNumberSpace) -> Vec<u8> {
        self.crypto_recv_mut(space).read()
    }

    /// Whether the peer has confirmed the handshake (HANDSHAKE_DONE received).
    pub fn handshake_confirmed(&self) -> bool {
        self.handshake_confirmed
    }

    /// The current connection lifecycle state.
    pub fn lifecycle(&self) -> &ConnectionLifecycle {
        &self.lifecycle
    }

    /// Whether the idle timeout has elapsed at `now` (RFC 9000 §10.1). When this
    /// returns `true` the connection is silently discarded — the driver drops the
    /// connection object and sends nothing further. Consults the same
    /// [`idle_deadline`](super::lifecycle::ConnectionLifecycle::idle_deadline) the
    /// [`QuicConnection::refresh_timers`] idle timer was armed from, using the
    /// connection's current probe timeout as the floor.
    pub fn is_idle_expired(&self, now: Instant) -> bool {
        self.lifecycle.is_idle_expired(now, self.pto)
    }

    /// Drive the path-validation abandon timer (RFC 9000 §8.2.4): if a validation
    /// is in progress and its deadline has elapsed at `now`, abandon it (the path
    /// transitions to failed) and return `true`; otherwise leave the state
    /// unchanged and return `false`.
    pub fn on_path_validation_timeout(&mut self, now: Instant) -> bool {
        self.path.on_timeout(now)
    }

    /// Drive the closing / draining period end (RFC 9000 §10.2): if the close
    /// deadline has elapsed at `now`, discard the connection state (moving it to
    /// [`ConnState::Closed`](super::lifecycle::ConnState::Closed)). Returns whether
    /// the connection has reached the closed state, so the driver can stop.
    pub fn on_close_timeout(&mut self, now: Instant) -> bool {
        self.lifecycle.discard(now);
        self.lifecycle.is_closed()
    }

    /// The peer's connection-ID set (the IDs we stamp on outgoing packets).
    pub fn remote_conn_ids(&self) -> &RemoteConnIds {
        &self.remote_cids
    }

    /// Our send-side view of the peer's connection-wide flow-control limit.
    pub fn send_flow(&self) -> &SendConnFlow {
        &self.send_flow
    }

    /// The path validator driving connection migration and path challenges.
    pub fn path(&self) -> &PathValidator {
        &self.path
    }

    /// The anti-amplification limit on the peer's address (RFC 9000 §8.1).
    pub fn anti_amplification(&self) -> &AntiAmplificationLimit {
        &self.anti_amplification
    }

    /// The bidirectional / unidirectional stream-count limit we may open under.
    pub fn send_stream_limit(&self, dir: StreamDir) -> &SendStreamLimit {
        match dir {
            StreamDir::Bidi => &self.send_bidi_limit,
            StreamDir::Uni => &self.send_uni_limit,
        }
    }

    /// The acknowledgement generator for `space`.
    fn ack_generator(&self, space: PacketNumberSpace) -> &AckGenerator {
        match space {
            PacketNumberSpace::Initial => &self.ack_initial,
            PacketNumberSpace::Handshake => &self.ack_handshake,
            PacketNumberSpace::ApplicationData => &self.ack_app,
        }
    }

    /// The acknowledgement generator for `space`, mutably.
    fn ack_generator_mut(&mut self, space: PacketNumberSpace) -> &mut AckGenerator {
        match space {
            PacketNumberSpace::Initial => &mut self.ack_initial,
            PacketNumberSpace::Handshake => &mut self.ack_handshake,
            PacketNumberSpace::ApplicationData => &mut self.ack_app,
        }
    }

    /// The CRYPTO reassembly buffer for `space`'s encryption level, mutably.
    fn crypto_recv_mut(&mut self, space: PacketNumberSpace) -> &mut CryptoRecvStream {
        match space {
            PacketNumberSpace::Initial => &mut self.crypto_initial,
            PacketNumberSpace::Handshake => &mut self.crypto_handshake,
            PacketNumberSpace::ApplicationData => &mut self.crypto_app,
        }
    }
}

/// Whether `frame` makes a packet ack-eliciting: any frame other than ACK,
/// PADDING, or CONNECTION_CLOSE (RFC 9002 §2).
fn is_ack_eliciting(frame: &Frame) -> bool {
    !matches!(
        frame,
        Frame::Ack { .. } | Frame::Padding(_) | Frame::ConnectionClose { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::lifecycle::ConnState;
    use crate::h3::quic_frame::STATELESS_RESET_TOKEN_LEN;
    use crate::h3::path_validation::PathState;

    fn config() -> ConnectionConfig {
        ConnectionConfig {
            peer_initial_cid: vec![0xAA; 8],
            local_initial_cid: vec![0xBB; 8],
            active_connection_id_limit: 4,
            peer_active_connection_id_limit: 4,
            peer_initial_max_data: 1_000,
            peer_initial_max_streams_bidi: 3,
            peer_initial_max_streams_uni: 3,
            pto: Duration::from_millis(100),
        }
    }

    fn conn(now: Instant) -> QuicConnection {
        QuicConnection::new_client(config(), now)
    }

    #[test]
    fn ping_is_ack_eliciting_and_arms_ack() {
        let now = Instant::now();
        let mut c = conn(now);
        let fx = c
            .process_packet(PacketNumberSpace::ApplicationData, 0, &[Frame::Ping], now)
            .unwrap();
        assert!(fx.ack_eliciting);
        // A first ack-eliciting packet in the App space owes a delayed ACK.
        assert!(matches!(
            c.ack_urgency(PacketNumberSpace::ApplicationData),
            AckUrgency::Delayed(_)
        ));
    }

    #[test]
    fn ack_only_packet_is_not_ack_eliciting() {
        let now = Instant::now();
        let mut c = conn(now);
        let ack = Frame::Ack {
            largest_acked: 5,
            ack_delay: 0,
            first_ack_range: 0,
            ranges: vec![],
            ecn: None,
        };
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                1,
                std::slice::from_ref(&ack),
                now,
            )
            .unwrap();
        assert!(!fx.ack_eliciting);
        assert_eq!(fx.deferred, vec![ack]);
        assert_eq!(
            c.ack_urgency(PacketNumberSpace::ApplicationData),
            AckUrgency::None
        );
    }

    #[test]
    fn crypto_frame_reassembles_per_space() {
        let now = Instant::now();
        let mut c = conn(now);
        c.process_packet(
            PacketNumberSpace::Initial,
            0,
            &[Frame::Crypto {
                offset: 0,
                data: b"hello".to_vec(),
            }],
            now,
        )
        .unwrap();
        assert_eq!(c.read_crypto(PacketNumberSpace::Initial), b"hello");
        // A different space's buffer is independent.
        assert!(c.read_crypto(PacketNumberSpace::Handshake).is_empty());
    }

    #[test]
    fn crypto_beyond_buffer_bound_is_a_connection_error() {
        let now = Instant::now();
        let mut c = conn(now);
        let err = c
            .process_packet(
                PacketNumberSpace::Initial,
                0,
                &[Frame::Crypto {
                    offset: 100_000,
                    data: b"x".to_vec(),
                }],
                now,
            )
            .unwrap_err();
        assert_eq!(err.code(), crate::h3::crypto_stream::CRYPTO_BUFFER_EXCEEDED);
        assert!(matches!(err, ProcessError::Crypto(_)));
    }

    #[test]
    fn max_data_raises_send_budget() {
        let now = Instant::now();
        let mut c = conn(now);
        assert_eq!(c.send_flow().max_data(), 1_000);
        c.process_packet(
            PacketNumberSpace::ApplicationData,
            0,
            &[Frame::MaxData(5_000)],
            now,
        )
        .unwrap();
        assert_eq!(c.send_flow().max_data(), 5_000);
    }

    #[test]
    fn max_streams_raises_the_right_axis() {
        let now = Instant::now();
        let mut c = conn(now);
        c.process_packet(
            PacketNumberSpace::ApplicationData,
            0,
            &[
                Frame::MaxStreams { bidi: true, max: 10 },
                Frame::MaxStreams { bidi: false, max: 7 },
            ],
            now,
        )
        .unwrap();
        assert_eq!(c.send_stream_limit(StreamDir::Bidi).max_streams(), 10);
        assert_eq!(c.send_stream_limit(StreamDir::Uni).max_streams(), 7);
    }

    #[test]
    fn new_connection_id_is_recorded() {
        let now = Instant::now();
        let mut c = conn(now);
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::NewConnectionId {
                    sequence_number: 1,
                    retire_prior_to: 0,
                    connection_id: vec![0xCC; 8],
                    stateless_reset_token: [0x11; STATELESS_RESET_TOKEN_LEN],
                }],
                now,
            )
            .unwrap();
        assert!(fx.retire_connection_ids.is_empty());
        assert_eq!(c.remote_conn_ids().active_count(), 2);
    }

    #[test]
    fn new_connection_id_retire_prior_to_reports_retired() {
        let now = Instant::now();
        let mut c = conn(now);
        // Retire the seed (sequence 0) by delivering seq 1 with retire_prior_to 1.
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::NewConnectionId {
                    sequence_number: 1,
                    retire_prior_to: 1,
                    connection_id: vec![0xCC; 8],
                    stateless_reset_token: [0x11; STATELESS_RESET_TOKEN_LEN],
                }],
                now,
            )
            .unwrap();
        assert_eq!(fx.retire_connection_ids, vec![0]);
    }

    #[test]
    fn malformed_new_connection_id_is_a_frame_encoding_error() {
        let now = Instant::now();
        let mut c = conn(now);
        let err = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::NewConnectionId {
                    sequence_number: 1,
                    retire_prior_to: 2, // > sequence_number → malformed
                    connection_id: vec![0xCC; 8],
                    stateless_reset_token: [0x11; STATELESS_RESET_TOKEN_LEN],
                }],
                now,
            )
            .unwrap_err();
        assert!(matches!(err, ProcessError::ConnId(ConnIdError::Malformed)));
    }

    #[test]
    fn retire_of_unissued_local_id_is_a_protocol_violation() {
        let now = Instant::now();
        let mut c = conn(now);
        // We only issued sequence 0; retiring 5 is a protocol violation.
        let err = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::RetireConnectionId(5)],
                now,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            ProcessError::ConnId(ConnIdError::SequenceConflict)
        ));
    }

    #[test]
    fn path_challenge_is_echoed_as_response() {
        let now = Instant::now();
        let mut c = conn(now);
        let data = [0x42; 8];
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::PathChallenge(data)],
                now,
            )
            .unwrap();
        assert_eq!(fx.responses, vec![Frame::PathResponse(data)]);
    }

    #[test]
    fn path_response_validates_and_lifts_anti_amplification() {
        let now = Instant::now();
        let mut c = conn(now);
        // Arm a challenge so a matching response validates.
        let mut challenge_bytes = [0u8; 8];
        challenge_bytes[0] = 9;
        let frame = c
            .send_path_challenge(challenge_bytes, now)
            .expect("challenge frame");
        let data = match frame {
            Frame::PathChallenge(d) => d,
            other => panic!("expected PATH_CHALLENGE, got {other:?}"),
        };
        assert!(!c.anti_amplification().is_validated());
        c.process_packet(
            PacketNumberSpace::ApplicationData,
            0,
            &[Frame::PathResponse(data)],
            now,
        )
        .unwrap();
        assert_eq!(c.path().state(), PathState::Validated);
        assert!(c.anti_amplification().is_validated());
    }

    #[test]
    fn connection_close_enters_draining() {
        let now = Instant::now();
        let mut c = conn(now);
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::ConnectionClose {
                    error_code: 0,
                    frame_type: None,
                    reason: b"bye".to_vec(),
                }],
                now,
            )
            .unwrap();
        assert!(fx.peer_closed);
        assert_eq!(c.lifecycle().state(), ConnState::Draining);
        // A CONNECTION_CLOSE-only packet is not ack-eliciting (RFC 9002 §2).
        assert!(!fx.ack_eliciting);
    }

    #[test]
    fn handshake_done_confirms_and_validates() {
        let now = Instant::now();
        let mut c = conn(now);
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[Frame::HandshakeDone],
                now,
            )
            .unwrap();
        assert!(fx.handshake_confirmed);
        assert!(c.handshake_confirmed());
        assert!(c.anti_amplification().is_validated());
    }

    #[test]
    fn stream_frames_are_deferred_in_order() {
        let now = Instant::now();
        let mut c = conn(now);
        let stream = Frame::Stream {
            stream_id: 0,
            offset: 0,
            fin: true,
            data: b"body".to_vec(),
        };
        let reset = Frame::ResetStream {
            stream_id: 4,
            app_error_code: 0,
            final_size: 0,
        };
        let fx = c
            .process_packet(
                PacketNumberSpace::ApplicationData,
                0,
                &[stream.clone(), reset.clone()],
                now,
            )
            .unwrap();
        assert_eq!(fx.deferred, vec![stream, reset]);
        assert!(fx.ack_eliciting);
    }

    #[test]
    fn on_datagram_received_credits_anti_amplification() {
        let now = Instant::now();
        let mut c = conn(now);
        // Before receiving anything the peer's address is unvalidated and the
        // send allowance is zero.
        assert_eq!(c.anti_amplification().send_allowance(), Some(0));
        c.on_datagram_received(1_200, now);
        // Now three times the received bytes may be sent.
        assert_eq!(c.anti_amplification().send_allowance(), Some(3_600));
    }

    #[test]
    fn refresh_timers_folds_ack_and_idle_deadlines() {
        let now = Instant::now();
        let mut c = conn(now);
        c.set_idle_timeout(Some(Duration::from_secs(30)));
        // An ack-eliciting App packet arms a delayed ACK.
        c.process_packet(PacketNumberSpace::ApplicationData, 0, &[Frame::Ping], now)
            .unwrap();
        let mut timers = ConnectionTimers::new();
        c.refresh_timers(&mut timers);
        assert!(
            timers
                .ack_delay_deadline(PacketNumberSpace::ApplicationData)
                .is_some()
        );
        assert!(timers.idle_timeout_deadline().is_some());
    }

    #[test]
    fn generate_ack_after_receipt() {
        let now = Instant::now();
        let mut c = conn(now);
        c.process_packet(PacketNumberSpace::Handshake, 7, &[Frame::Ping], now)
            .unwrap();
        let ack = c.generate_ack(PacketNumberSpace::Handshake, now, 3);
        match ack {
            Some(Frame::Ack { largest_acked, .. }) => assert_eq!(largest_acked, 7),
            other => panic!("expected ACK for pn 7, got {other:?}"),
        }
    }
}
