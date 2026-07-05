//! QUIC event-loop wait (RFC 9000 §10.1, §13.2.1; RFC 9002 §6.2): the glue that
//! ties the [`timer::ConnectionTimers`] scheduler to the [`udp::DatagramTransport`]
//! read timeout.
//!
//! Every earlier slice is either a pure state machine ([`timer`]) or the raw
//! datagram seam ([`udp`]); this slice is the single place that turns "the next
//! QUIC deadline" into "block the socket for exactly this long, then tell me why
//! I woke". It is the wait *iteration* of the connection's single-threaded event
//! loop — not the full loop, which also composes the transport state machines
//! (a later slice). One turn of that loop is:
//!
//! 1. Refresh every state machine's deadline into [`timer::ConnectionTimers`].
//! 2. Ask the scheduler for the earliest deadline
//!    ([`timer::ConnectionTimers::next`]).
//! 3. Convert that deadline into a socket read timeout
//!    ([`udp::recv_timeout`]) and block in [`udp::DatagramTransport::recv`].
//! 4. Either a datagram arrived (process it) or the timeout elapsed (the earliest
//!    QUIC timer is due — drive whichever machines
//!    [`timer::ConnectionTimers::fired`] reports).
//!
//! Steps 2–4 are exactly [`DatagramEventLoop::wait`]. It owns the transport and a
//! receive buffer sized to the maximum QUIC datagram
//! ([`udp::MAX_DATAGRAM_SIZE`]) so the caller never re-allocates per turn, and it
//! reports the wake as a [`Wakeup`].
//!
//! ## The clock boundary
//!
//! Like every other slice the *block duration* is computed from a caller-supplied
//! `now`, so a test drives the wait with a [`udp::MockDatagramTransport`] and a
//! synthetic clock with no real socket. Only the OS timer itself — the read
//! timeout the kernel counts down — is real. On a [`Wakeup::TimerExpired`] the
//! caller reads the wall clock once (`Instant::now()`) and passes it to
//! [`timer::ConnectionTimers::fired`] to learn which deadlines actually elapsed;
//! keeping that final `fired` call in the caller (rather than inside `wait`) is
//! what lets this module stay clock-free and deterministically testable.

use std::io;
use std::time::{Duration, Instant};

use super::timer::ConnectionTimers;
use super::udp::{DatagramTransport, MAX_DATAGRAM_SIZE, recv_timed_out, recv_timeout};

/// Why one [`DatagramEventLoop::wait`] returned.
///
/// A QUIC event loop alternates between processing inbound datagrams and firing
/// due timers; this enum is which of the two the wait produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wakeup {
    /// An inbound datagram of this many bytes was read into the loop's receive
    /// buffer. Retrieve the bytes with [`DatagramEventLoop::datagram`].
    Datagram(usize),
    /// No datagram arrived before the earliest armed deadline elapsed. The caller
    /// now drives every timer [`ConnectionTimers::fired`] reports for the current
    /// instant.
    TimerExpired,
}

/// The read timeout the next [`DatagramTransport::recv`] should block for, derived
/// from the connection's timers and the current instant.
///
/// A pure helper composing [`ConnectionTimers::next`] with [`recv_timeout`]:
///
/// - no timer armed → `None` (block until a datagram arrives),
/// - earliest deadline still in the future → `Some(deadline - now)`,
/// - earliest deadline already due → `Some(Duration::ZERO)` (poll without
///   blocking so the timer fires this iteration).
pub fn next_read_timeout(timers: &ConnectionTimers, now: Instant) -> Option<Duration> {
    recv_timeout(timers.next().map(|armed| armed.deadline), now)
}

/// One turn's receive side of the QUIC connection event loop: a
/// [`DatagramTransport`] plus a reusable maximum-size receive buffer.
///
/// The loop owns its transport so [`DatagramEventLoop::wait`] can arm the read
/// timeout and receive in one call; the buffer is allocated once at construction
/// ([`MAX_DATAGRAM_SIZE`]) so no per-turn allocation occurs. The connection
/// state machines are *not* owned here — composing them over this wait is the
/// connection-engine slice; this type is only the socket-and-timer wait.
#[derive(Debug)]
pub struct DatagramEventLoop<T: DatagramTransport> {
    /// The datagram transport this loop receives from and sends over.
    transport: T,
    /// The reusable receive buffer, sized to never truncate a QUIC datagram.
    buf: Vec<u8>,
}

impl<T: DatagramTransport> DatagramEventLoop<T> {
    /// Wraps `transport` in an event loop with a fresh maximum-size receive
    /// buffer.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            buf: vec![0u8; MAX_DATAGRAM_SIZE],
        }
    }

    /// A shared reference to the underlying transport (e.g. to read
    /// [`DatagramTransport::peer_addr`]).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// A mutable reference to the underlying transport, e.g. to
    /// [`DatagramTransport::send`] an outgoing datagram between waits.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Consumes the loop and returns the owned transport.
    pub fn into_transport(self) -> T {
        self.transport
    }

    /// The bytes of the datagram reported by the most recent
    /// [`Wakeup::Datagram(n)`](Wakeup::Datagram).
    ///
    /// `n` is the length that [`Wakeup::Datagram`] carried; the returned slice is
    /// the front `n` bytes of the receive buffer. Calling this with an `n` from a
    /// stale wake (or larger than [`MAX_DATAGRAM_SIZE`]) panics on the slice
    /// bound, matching the contract that `n` comes straight from the last wake.
    pub fn datagram(&self, n: usize) -> &[u8] {
        &self.buf[..n]
    }

    /// Blocks for one event-loop turn: arm the socket read timeout from `timers`
    /// and `now`, then receive.
    ///
    /// Returns [`Wakeup::Datagram(n)`](Wakeup::Datagram) when a datagram arrived
    /// (its bytes are then [`DatagramEventLoop::datagram`]), or
    /// [`Wakeup::TimerExpired`] when the earliest armed deadline elapsed with no
    /// datagram — the portable "read timed out" signal ([`recv_timed_out`]). Any
    /// other socket error is propagated. When no timer is armed the read timeout
    /// is `None` and the wait blocks until a datagram arrives (a real socket never
    /// times out then; a [`super::udp::MockDatagramTransport`] with an empty queue
    /// still reports the timeout signal, so tests can exercise the timer path
    /// without a clock).
    pub fn wait(&mut self, timers: &ConnectionTimers, now: Instant) -> io::Result<Wakeup> {
        self.transport
            .set_read_timeout(next_read_timeout(timers, now))?;
        match self.transport.recv(&mut self.buf) {
            Ok(n) => Ok(Wakeup::Datagram(n)),
            Err(e) if recv_timed_out(&e) => Ok(Wakeup::TimerExpired),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::loss::PacketNumberSpace;
    use crate::h3::pto::LossTimer;
    use crate::h3::udp::MockDatagramTransport;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn mock() -> MockDatagramTransport {
        MockDatagramTransport::new(loopback(1), loopback(2))
    }

    /// A transport whose `recv` always fails with a hard, non-timeout error, to
    /// prove [`DatagramEventLoop::wait`] propagates real socket failures rather
    /// than mistaking them for a timeout.
    #[derive(Debug)]
    struct FailingTransport {
        armed: Option<Duration>,
    }

    impl DatagramTransport for FailingTransport {
        fn send(&mut self, _datagram: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn recv(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "peer reset",
            ))
        }
        fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
            self.armed = timeout;
            Ok(())
        }
        fn local_addr(&self) -> io::Result<SocketAddr> {
            Ok(loopback(1))
        }
        fn peer_addr(&self) -> io::Result<SocketAddr> {
            Ok(loopback(2))
        }
    }

    // ---- next_read_timeout ----------------------------------------------

    #[test]
    fn next_read_timeout_none_when_no_timer_armed() {
        let timers = ConnectionTimers::new();
        assert_eq!(next_read_timeout(&timers, Instant::now()), None);
    }

    #[test]
    fn next_read_timeout_is_remaining_for_future_deadline() {
        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now + Duration::from_millis(200)));
        assert_eq!(
            next_read_timeout(&timers, now),
            Some(Duration::from_millis(200))
        );
    }

    #[test]
    fn next_read_timeout_is_zero_for_elapsed_deadline() {
        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now - Duration::from_millis(5)));
        assert_eq!(next_read_timeout(&timers, now), Some(Duration::ZERO));
    }

    #[test]
    fn next_read_timeout_tracks_earliest_deadline() {
        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(90)));
        timers.set_ack_delay(
            PacketNumberSpace::ApplicationData,
            crate::h3::ack::AckUrgency::Delayed(now + Duration::from_millis(30)),
        );
        // The 30ms ACK deadline is earliest, so that is the block duration.
        assert_eq!(
            next_read_timeout(&timers, now),
            Some(Duration::from_millis(30))
        );
    }

    // ---- wait: datagram path --------------------------------------------

    #[test]
    fn wait_returns_datagram_when_one_is_queued() {
        let mut transport = mock();
        transport.push_inbound(b"quic-datagram".to_vec());
        let mut ev = DatagramEventLoop::new(transport);

        let timers = ConnectionTimers::new();
        let wake = ev.wait(&timers, Instant::now()).unwrap();
        assert_eq!(wake, Wakeup::Datagram(13));
        match wake {
            Wakeup::Datagram(n) => assert_eq!(ev.datagram(n), b"quic-datagram"),
            other => panic!("expected datagram, got {other:?}"),
        }
    }

    #[test]
    fn wait_arms_read_timeout_from_earliest_deadline() {
        let mut transport = mock();
        transport.push_inbound(b"x".to_vec());
        let mut ev = DatagramEventLoop::new(transport);

        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now + Duration::from_millis(75)));
        ev.wait(&timers, now).unwrap();

        assert_eq!(
            ev.transport().read_timeout(),
            Some(Duration::from_millis(75)),
            "wait must arm the socket read timeout from the earliest timer"
        );
    }

    #[test]
    fn wait_arms_no_timeout_when_no_timer() {
        let mut transport = mock();
        transport.push_inbound(b"x".to_vec());
        let mut ev = DatagramEventLoop::new(transport);

        ev.wait(&ConnectionTimers::new(), Instant::now()).unwrap();
        assert_eq!(
            ev.transport().read_timeout(),
            None,
            "no timer armed → block indefinitely"
        );
    }

    #[test]
    fn wait_arms_zero_timeout_when_deadline_already_due() {
        let mut transport = mock();
        transport.push_inbound(b"x".to_vec());
        let mut ev = DatagramEventLoop::new(transport);

        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now - Duration::from_millis(1)));
        ev.wait(&timers, now).unwrap();
        assert_eq!(ev.transport().read_timeout(), Some(Duration::ZERO));
    }

    // ---- wait: timer path -----------------------------------------------

    #[test]
    fn wait_reports_timer_expired_on_empty_queue() {
        // The mock reports WouldBlock when empty — exactly what a real socket does
        // when its read timeout elapses — so the loop takes the timer branch.
        let mut ev = DatagramEventLoop::new(mock());
        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_loss_detection(LossTimer::Armed(now + Duration::from_millis(10)));

        assert_eq!(ev.wait(&timers, now).unwrap(), Wakeup::TimerExpired);
    }

    #[test]
    fn timer_expired_then_fired_reports_the_due_timer() {
        // End-to-end: the wait times out, and the same timers report the elapsed
        // deadline the caller must drive.
        let mut ev = DatagramEventLoop::new(mock());
        let now = Instant::now();
        let mut timers = ConnectionTimers::new();
        timers.set_idle_timeout(Some(now + Duration::from_millis(5)));

        assert_eq!(ev.wait(&timers, now).unwrap(), Wakeup::TimerExpired);
        // The caller reads the clock after waking; here the deadline is in the
        // past relative to a later instant.
        let fired = timers.fired(now + Duration::from_millis(20));
        assert_eq!(fired, vec![crate::h3::timer::TimerKind::IdleTimeout]);
    }

    // ---- wait: error propagation ----------------------------------------

    #[test]
    fn wait_propagates_non_timeout_error() {
        let mut ev = DatagramEventLoop::new(FailingTransport { armed: None });
        let err = ev
            .wait(&ConnectionTimers::new(), Instant::now())
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
    }

    // ---- buffer reuse across turns --------------------------------------

    #[test]
    fn wait_reuses_buffer_across_datagrams() {
        let mut transport = mock();
        transport.push_inbound(b"first-datagram".to_vec());
        transport.push_inbound(b"two".to_vec());
        let mut ev = DatagramEventLoop::new(transport);
        let timers = ConnectionTimers::new();

        let w1 = ev.wait(&timers, Instant::now()).unwrap();
        assert_eq!(w1, Wakeup::Datagram(14));
        assert_eq!(ev.datagram(14), b"first-datagram");

        let w2 = ev.wait(&timers, Instant::now()).unwrap();
        assert_eq!(w2, Wakeup::Datagram(3));
        // The shorter second datagram only exposes its own bytes; stale trailing
        // bytes of the buffer are never handed out because `n` bounds the slice.
        assert_eq!(ev.datagram(3), b"two");
    }

    // ---- transport accessors --------------------------------------------

    #[test]
    fn transport_mut_can_send_between_waits() {
        let mut ev = DatagramEventLoop::new(mock());
        ev.transport_mut().send(b"outgoing").unwrap();
        assert_eq!(ev.transport().sent, vec![b"outgoing".to_vec()]);
    }

    #[test]
    fn into_transport_returns_the_owned_transport() {
        let mut transport = mock();
        transport.push_inbound(b"pending".to_vec());
        let ev = DatagramEventLoop::new(transport);
        let recovered = ev.into_transport();
        assert_eq!(recovered.inbound_len(), 1);
    }
}
