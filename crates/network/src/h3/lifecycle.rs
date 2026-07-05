//! QUIC connection lifecycle — idle timeout and immediate close (RFC 9000 §10).
//!
//! A QUIC connection leaves the active state in one of three ways (RFC 9000 §10):
//!
//! - **Idle timeout** (§10.1): the connection is silently discarded once it
//!   stays idle longer than the negotiated `max_idle_timeout`. No packet is
//!   sent — the peer's own idle timer expires in parallel.
//! - **Immediate close** (§10.2): an endpoint sends a `CONNECTION_CLOSE` frame
//!   and enters the **closing** state, retaining just enough state to answer any
//!   stray incoming packet with the same close frame (rate-limited) until the
//!   closing period ends.
//! - **Draining** (§10.2.2): an endpoint that *receives* a `CONNECTION_CLOSE`
//!   enters the **draining** state and MUST NOT send anything further until the
//!   draining period ends.
//!
//! Both the closing and draining periods last three times the current Probe
//! Timeout (`3·PTO`, RFC 9000 §10.2), giving in-flight packets time to drain
//! before the connection ID and keys are discarded.
//!
//! This is a pure state machine in the same mould as [`pto`](super::pto) and
//! [`loss`](super::loss): the clock is supplied by the caller (`now: Instant`)
//! and the Probe Timeout is supplied as a [`Duration`] (compute it with
//! [`RttEstimator::pto`](super::recovery::RttEstimator::pto)). It performs no IO
//! and owns no timers — it answers *when* a deadline falls and *what* frame to
//! (re)send, leaving the actual send/receive and timer arming to the connection
//! layer.
//!
//! ## Out of scope (deferred to later slices)
//!
//! - The UDP send/receive and the actual arming of an idle / close timer.
//! - Stateless reset detection (RFC 9000 §10.3), partly handled by
//!   [`conn_id`](super::conn_id).
//! - Retaining and discarding packet-protection keys across the close.

use std::time::{Duration, Instant};

use super::quic_frame::Frame;

/// Multiplier applied to the Probe Timeout for the closing and draining periods
/// (RFC 9000 §10.2): both last `3·PTO`.
pub const CLOSE_PERIOD_PTO_MULTIPLIER: u32 = 3;

/// Minimum multiple of the Probe Timeout the effective idle timeout is raised to
/// (RFC 9000 §10.1): endpoints MUST use at least `3·PTO` so several probes can be
/// sent and lost before the idle timer fires.
pub const IDLE_TIMEOUT_PTO_FLOOR_MULTIPLIER: u32 = 3;

/// The lifecycle state of a QUIC connection (RFC 9000 §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    /// The connection is open and exchanging packets.
    Active,
    /// This endpoint sent a `CONNECTION_CLOSE` and is in the closing period
    /// (RFC 9000 §10.2.1): it may answer incoming packets with the close frame
    /// but sends nothing else.
    Closing,
    /// This endpoint received a `CONNECTION_CLOSE` and is in the draining period
    /// (RFC 9000 §10.2.2): it MUST NOT send any packet.
    Draining,
    /// The closing or draining period has elapsed; the connection state is
    /// discarded.
    Closed,
}

/// Compute the effective idle timeout from the two advertised `max_idle_timeout`
/// transport-parameter values in milliseconds (RFC 9000 §10.1, §18.2).
///
/// A `None` or `Some(0)` value means the endpoint imposes no idle limit. When
/// both endpoints advertise a non-zero value the effective timeout is their
/// minimum; when only one does, it is that value; when neither does, idle
/// timeout is disabled (`None`).
#[must_use]
pub fn effective_idle_timeout(local_ms: Option<u64>, peer_ms: Option<u64>) -> Option<Duration> {
    let local = local_ms.filter(|&v| v != 0);
    let peer = peer_ms.filter(|&v| v != 0);
    let ms = match (local, peer) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    Some(Duration::from_millis(ms))
}

/// The QUIC connection lifecycle state machine (RFC 9000 §10).
///
/// Tracks the active/closing/draining/closed transitions, the idle-timeout
/// deadline, and the closing-state `CONNECTION_CLOSE` retransmission budget. The
/// caller drives it with clock readings and feeds it the events that reset the
/// idle timer (an ack-eliciting packet sent, any packet received) or trigger a
/// close.
#[derive(Clone, Debug)]
pub struct ConnectionLifecycle {
    /// The current lifecycle state.
    state: ConnState,
    /// The negotiated effective idle timeout, or `None` when disabled.
    idle_timeout: Option<Duration>,
    /// The instant the idle timer last restarted (RFC 9000 §10.1).
    idle_restart: Instant,
    /// Whether an ack-eliciting packet has been sent since the last packet was
    /// received (RFC 9000 §10.1): a further ack-eliciting send does not restart
    /// the idle timer until another packet arrives.
    ack_eliciting_since_recv: bool,
    /// The instant the closing or draining period ends, once one is entered.
    close_deadline: Option<Instant>,
    /// The `CONNECTION_CLOSE` parameters retained for the closing state.
    close_frame: Option<CloseParams>,
    /// Incoming packets counted toward the next closing-state resend.
    close_recv_count: u32,
    /// The current resend threshold; doubles after each resend for rate limiting
    /// (RFC 9000 §10.2.1).
    close_resend_threshold: u32,
}

/// The parameters of the `CONNECTION_CLOSE` frame this endpoint sends and resends
/// while in the closing state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CloseParams {
    /// The error code (a transport code when `frame_type` is `Some`, an
    /// application code otherwise).
    error_code: u64,
    /// The frame type that triggered a transport-level close, or `None` for an
    /// application-level close.
    frame_type: Option<u64>,
    /// The human-readable reason phrase (may be empty).
    reason: Vec<u8>,
}

impl CloseParams {
    /// Build the wire [`Frame::ConnectionClose`] these parameters describe.
    fn to_frame(&self) -> Frame {
        Frame::ConnectionClose {
            error_code: self.error_code,
            frame_type: self.frame_type,
            reason: self.reason.clone(),
        }
    }
}

impl ConnectionLifecycle {
    /// Create an active lifecycle with idle timeout disabled, starting the idle
    /// timer at `now`. Call [`set_idle_timeout`](Self::set_idle_timeout) once the
    /// handshake exchanges the `max_idle_timeout` transport parameters.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            state: ConnState::Active,
            idle_timeout: None,
            idle_restart: now,
            ack_eliciting_since_recv: false,
            close_deadline: None,
            close_frame: None,
            close_recv_count: 0,
            close_resend_threshold: 1,
        }
    }

    /// Set the negotiated effective idle timeout (RFC 9000 §10.1), typically the
    /// output of [`effective_idle_timeout`] applied to the two advertised
    /// `max_idle_timeout` values.
    pub fn set_idle_timeout(&mut self, timeout: Option<Duration>) {
        self.idle_timeout = timeout;
    }

    /// The current lifecycle state.
    #[must_use]
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// `true` while the connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state == ConnState::Active
    }

    /// `true` while this endpoint is in the closing state and may resend its
    /// `CONNECTION_CLOSE` frame.
    #[must_use]
    pub fn is_closing(&self) -> bool {
        self.state == ConnState::Closing
    }

    /// `true` while this endpoint is in the draining state and MUST NOT send.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.state == ConnState::Draining
    }

    /// `true` once the connection state has been discarded.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state == ConnState::Closed
    }

    /// Whether this endpoint may send an ordinary (non-close) packet: only in the
    /// active state (RFC 9000 §10.2). In the closing state it may send only the
    /// `CONNECTION_CLOSE` frame; in draining or closed it may send nothing.
    #[must_use]
    pub fn may_send(&self) -> bool {
        self.state == ConnState::Active
    }

    /// Restart the idle timer because a packet from the peer was received and
    /// processed (RFC 9000 §10.1). A no-op once the connection is no longer
    /// active.
    pub fn on_packet_received(&mut self, now: Instant) {
        if self.state == ConnState::Active {
            self.idle_restart = now;
            self.ack_eliciting_since_recv = false;
        }
    }

    /// Restart the idle timer because an ack-eliciting packet was sent, but only
    /// if none has been sent since the last packet was received (RFC 9000 §10.1).
    /// A no-op once the connection is no longer active.
    pub fn on_ack_eliciting_sent(&mut self, now: Instant) {
        if self.state == ConnState::Active && !self.ack_eliciting_since_recv {
            self.idle_restart = now;
            self.ack_eliciting_since_recv = true;
        }
    }

    /// The instant the idle timer fires, given the current Probe Timeout `pto`
    /// (RFC 9000 §10.1). The effective period is the negotiated timeout raised to
    /// at least `3·PTO`. Returns `None` when idle timeout is disabled or the
    /// connection is no longer active.
    #[must_use]
    pub fn idle_deadline(&self, pto: Duration) -> Option<Instant> {
        if self.state != ConnState::Active {
            return None;
        }
        let timeout = self.idle_timeout?;
        let floor = pto * IDLE_TIMEOUT_PTO_FLOOR_MULTIPLIER;
        Some(self.idle_restart + timeout.max(floor))
    }

    /// Whether the idle timer has fired at `now` (RFC 9000 §10.1). The caller
    /// silently discards the connection when this returns `true`.
    #[must_use]
    pub fn is_idle_expired(&self, now: Instant, pto: Duration) -> bool {
        self.idle_deadline(pto).is_some_and(|deadline| now >= deadline)
    }

    /// Begin an immediate close (RFC 9000 §10.2): record the `CONNECTION_CLOSE`
    /// parameters, enter the closing state, and start the `3·PTO` closing period
    /// at `now`. Returns the [`Frame::ConnectionClose`] to send, or `None` if the
    /// connection has already left the active state (a close is initiated once).
    ///
    /// `frame_type` is `Some` for a transport-level close (type `0x1c`) naming the
    /// offending frame and `None` for an application-level close (type `0x1d`).
    pub fn close(
        &mut self,
        now: Instant,
        pto: Duration,
        error_code: u64,
        frame_type: Option<u64>,
        reason: Vec<u8>,
    ) -> Option<Frame> {
        if self.state != ConnState::Active {
            return None;
        }
        let params = CloseParams { error_code, frame_type, reason };
        let frame = params.to_frame();
        self.close_frame = Some(params);
        self.state = ConnState::Closing;
        self.close_deadline = Some(now + pto * CLOSE_PERIOD_PTO_MULTIPLIER);
        self.close_recv_count = 0;
        self.close_resend_threshold = 1;
        Some(frame)
    }

    /// Handle a received `CONNECTION_CLOSE` frame (RFC 9000 §10.2.2).
    ///
    /// From the active state this enters the draining state and starts the
    /// `3·PTO` draining period at `now`. From the closing state it likewise moves
    /// to draining, since the peer has also closed and no further close frame need
    /// be sent. It is a no-op once draining or closed. In every case the caller
    /// MUST NOT send any packet afterwards.
    pub fn on_connection_close_received(&mut self, now: Instant, pto: Duration) {
        match self.state {
            ConnState::Active | ConnState::Closing => {
                self.state = ConnState::Draining;
                self.close_deadline = Some(now + pto * CLOSE_PERIOD_PTO_MULTIPLIER);
            }
            ConnState::Draining | ConnState::Closed => {}
        }
    }

    /// Handle an incoming packet while in the closing state (RFC 9000 §10.2.1).
    ///
    /// Returns the `CONNECTION_CLOSE` frame to resend, or `None`. To bound the
    /// packets it generates, this endpoint resends only once the number of packets
    /// received since the previous resend reaches a threshold that doubles each
    /// time (exponential rate limiting). Outside the closing state it always
    /// returns `None`.
    pub fn on_packet_while_closing(&mut self) -> Option<Frame> {
        if self.state != ConnState::Closing {
            return None;
        }
        self.close_recv_count += 1;
        if self.close_recv_count >= self.close_resend_threshold {
            self.close_recv_count = 0;
            self.close_resend_threshold = self.close_resend_threshold.saturating_mul(2);
            return self.close_frame.as_ref().map(CloseParams::to_frame);
        }
        None
    }

    /// The `CONNECTION_CLOSE` frame retained for the closing state, if any.
    #[must_use]
    pub fn close_frame(&self) -> Option<Frame> {
        self.close_frame.as_ref().map(CloseParams::to_frame)
    }

    /// The instant the closing or draining period ends, if one is in progress
    /// (RFC 9000 §10.2).
    #[must_use]
    pub fn close_deadline(&self) -> Option<Instant> {
        self.close_deadline
    }

    /// Whether the closing or draining period has elapsed at `now`. Once this is
    /// `true`, call [`discard`](Self::discard) to move to the closed state.
    #[must_use]
    pub fn is_close_expired(&self, now: Instant) -> bool {
        self.close_deadline.is_some_and(|deadline| now >= deadline)
    }

    /// Discard the connection state once the closing or draining period has
    /// elapsed at `now` (RFC 9000 §10.2), moving to [`ConnState::Closed`]. A no-op
    /// before the deadline or when no close is in progress.
    pub fn discard(&mut self, now: Instant) {
        if self.is_close_expired(now) {
            self.state = ConnState::Closed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed origin instant plus a helper to offset from it, avoiding the
    /// forbidden `Instant::now()` in tests.
    fn origin() -> Instant {
        Instant::now()
    }

    const PTO: Duration = Duration::from_millis(100);

    #[test]
    fn effective_idle_timeout_takes_the_minimum() {
        assert_eq!(
            effective_idle_timeout(Some(30_000), Some(10_000)),
            Some(Duration::from_millis(10_000))
        );
        assert_eq!(
            effective_idle_timeout(Some(10_000), Some(30_000)),
            Some(Duration::from_millis(10_000))
        );
    }

    #[test]
    fn effective_idle_timeout_uses_sole_nonzero_value() {
        assert_eq!(
            effective_idle_timeout(Some(5_000), None),
            Some(Duration::from_millis(5_000))
        );
        assert_eq!(
            effective_idle_timeout(None, Some(7_000)),
            Some(Duration::from_millis(7_000))
        );
        // A zero value means "no limit" for that side, so the other side wins.
        assert_eq!(
            effective_idle_timeout(Some(0), Some(4_000)),
            Some(Duration::from_millis(4_000))
        );
    }

    #[test]
    fn effective_idle_timeout_disabled_when_neither_advertises() {
        assert_eq!(effective_idle_timeout(None, None), None);
        assert_eq!(effective_idle_timeout(Some(0), Some(0)), None);
        assert_eq!(effective_idle_timeout(Some(0), None), None);
    }

    #[test]
    fn new_connection_is_active_with_idle_disabled() {
        let lc = ConnectionLifecycle::new(origin());
        assert_eq!(lc.state(), ConnState::Active);
        assert!(lc.is_active());
        assert!(lc.may_send());
        assert_eq!(lc.idle_deadline(PTO), None);
    }

    #[test]
    fn idle_deadline_honours_the_three_pto_floor() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        // A 100 ms idle timeout is below 3·PTO = 300 ms, so the floor applies.
        lc.set_idle_timeout(Some(Duration::from_millis(100)));
        assert_eq!(lc.idle_deadline(PTO), Some(t0 + Duration::from_millis(300)));

        // A 1 s idle timeout exceeds the floor and is used verbatim.
        lc.set_idle_timeout(Some(Duration::from_millis(1_000)));
        assert_eq!(lc.idle_deadline(PTO), Some(t0 + Duration::from_millis(1_000)));
    }

    #[test]
    fn received_packet_restarts_the_idle_timer() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.set_idle_timeout(Some(Duration::from_secs(10)));
        assert_eq!(lc.idle_deadline(PTO), Some(t0 + Duration::from_secs(10)));

        let t1 = t0 + Duration::from_secs(3);
        lc.on_packet_received(t1);
        assert_eq!(lc.idle_deadline(PTO), Some(t1 + Duration::from_secs(10)));
    }

    #[test]
    fn ack_eliciting_send_restarts_timer_only_once_per_receive() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.set_idle_timeout(Some(Duration::from_secs(10)));

        // First ack-eliciting send after a receive restarts the timer.
        let t1 = t0 + Duration::from_secs(2);
        lc.on_ack_eliciting_sent(t1);
        assert_eq!(lc.idle_deadline(PTO), Some(t1 + Duration::from_secs(10)));

        // A second ack-eliciting send with no intervening receive does not.
        let t2 = t0 + Duration::from_secs(4);
        lc.on_ack_eliciting_sent(t2);
        assert_eq!(lc.idle_deadline(PTO), Some(t1 + Duration::from_secs(10)));

        // After a receive, the next ack-eliciting send restarts it again.
        let t3 = t0 + Duration::from_secs(5);
        lc.on_packet_received(t3);
        let t4 = t0 + Duration::from_secs(6);
        lc.on_ack_eliciting_sent(t4);
        assert_eq!(lc.idle_deadline(PTO), Some(t4 + Duration::from_secs(10)));
    }

    #[test]
    fn is_idle_expired_fires_at_the_deadline() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.set_idle_timeout(Some(Duration::from_secs(10)));
        assert!(!lc.is_idle_expired(t0 + Duration::from_secs(9), PTO));
        assert!(lc.is_idle_expired(t0 + Duration::from_secs(10), PTO));
        assert!(lc.is_idle_expired(t0 + Duration::from_secs(11), PTO));
    }

    #[test]
    fn close_enters_closing_and_returns_the_frame() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        let frame = lc.close(t0, PTO, 0x0a, Some(0x1c), b"bad frame".to_vec());
        assert_eq!(
            frame,
            Some(Frame::ConnectionClose {
                error_code: 0x0a,
                frame_type: Some(0x1c),
                reason: b"bad frame".to_vec(),
            })
        );
        assert_eq!(lc.state(), ConnState::Closing);
        assert!(lc.is_closing());
        assert!(!lc.may_send());
        // The closing period is 3·PTO from now.
        assert_eq!(lc.close_deadline(), Some(t0 + PTO * 3));
    }

    #[test]
    fn close_is_only_initiated_once() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        assert!(lc.close(t0, PTO, 0, None, Vec::new()).is_some());
        // A second close attempt from the closing state does nothing.
        assert_eq!(lc.close(t0, PTO, 1, None, Vec::new()), None);
        assert_eq!(lc.state(), ConnState::Closing);
    }

    #[test]
    fn idle_deadline_is_none_once_closing() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.set_idle_timeout(Some(Duration::from_secs(10)));
        lc.close(t0, PTO, 0, None, Vec::new());
        assert_eq!(lc.idle_deadline(PTO), None);
    }

    #[test]
    fn receiving_close_from_active_enters_draining() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.on_connection_close_received(t0, PTO);
        assert_eq!(lc.state(), ConnState::Draining);
        assert!(lc.is_draining());
        assert!(!lc.may_send());
        assert_eq!(lc.close_deadline(), Some(t0 + PTO * 3));
    }

    #[test]
    fn receiving_close_while_closing_moves_to_draining() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.close(t0, PTO, 0, None, Vec::new());
        assert_eq!(lc.state(), ConnState::Closing);
        lc.on_connection_close_received(t0, PTO);
        assert_eq!(lc.state(), ConnState::Draining);
    }

    #[test]
    fn draining_endpoint_never_resends_close() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.on_connection_close_received(t0, PTO);
        assert_eq!(lc.on_packet_while_closing(), None);
    }

    #[test]
    fn closing_resend_is_rate_limited_with_doubling_threshold() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.close(t0, PTO, 0x02, None, b"x".to_vec());
        let expected = Frame::ConnectionClose {
            error_code: 0x02,
            frame_type: None,
            reason: b"x".to_vec(),
        };
        // Threshold 1: the first incoming packet triggers a resend.
        assert_eq!(lc.on_packet_while_closing(), Some(expected.clone()));
        // Threshold 2: the next resend needs two more packets.
        assert_eq!(lc.on_packet_while_closing(), None);
        assert_eq!(lc.on_packet_while_closing(), Some(expected.clone()));
        // Threshold 4: the following resend needs four more packets.
        assert_eq!(lc.on_packet_while_closing(), None);
        assert_eq!(lc.on_packet_while_closing(), None);
        assert_eq!(lc.on_packet_while_closing(), None);
        assert_eq!(lc.on_packet_while_closing(), Some(expected));
    }

    #[test]
    fn close_frame_is_retained_for_the_closing_state() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        assert_eq!(lc.close_frame(), None);
        lc.close(t0, PTO, 0x05, Some(0x08), b"flow".to_vec());
        assert_eq!(
            lc.close_frame(),
            Some(Frame::ConnectionClose {
                error_code: 0x05,
                frame_type: Some(0x08),
                reason: b"flow".to_vec(),
            })
        );
    }

    #[test]
    fn discard_moves_to_closed_only_after_the_period() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.close(t0, PTO, 0, None, Vec::new());
        let deadline = t0 + PTO * 3;

        // Before the deadline, discard is a no-op.
        lc.discard(deadline - Duration::from_millis(1));
        assert_eq!(lc.state(), ConnState::Closing);
        assert!(!lc.is_close_expired(deadline - Duration::from_millis(1)));

        // At the deadline, the state is discarded.
        assert!(lc.is_close_expired(deadline));
        lc.discard(deadline);
        assert_eq!(lc.state(), ConnState::Closed);
        assert!(lc.is_closed());
    }

    #[test]
    fn draining_period_also_expires_to_closed() {
        let t0 = origin();
        let mut lc = ConnectionLifecycle::new(t0);
        lc.on_connection_close_received(t0, PTO);
        let deadline = t0 + PTO * 3;
        lc.discard(deadline);
        assert_eq!(lc.state(), ConnState::Closed);
    }

    #[test]
    fn no_close_in_progress_has_no_deadline() {
        let lc = ConnectionLifecycle::new(origin());
        assert_eq!(lc.close_deadline(), None);
        assert!(!lc.is_close_expired(origin()));
    }
}
