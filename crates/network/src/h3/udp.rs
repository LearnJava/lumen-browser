//! QUIC datagram transport (RFC 9000 §5, §12.2): the UDP send/recv seam.
//!
//! Every prior slice is a pure, IO-free state machine or codec; this is the
//! first slice that touches an operating-system socket. It mirrors the way the
//! HTTP/2 connection ([`crate::h2::conn::H2Conn`]) abstracts its byte transport
//! behind `Read + Write`: the QUIC connection layer will be generic over a
//! [`DatagramTransport`] rather than owning a concrete socket, so the same
//! connection engine can run over a real [`UdpDatagram`] in production and a
//! [`MockDatagramTransport`] in a deterministic test.
//!
//! QUIC is a *datagram* protocol, not a byte stream: a single UDP datagram
//! carries one or more coalesced QUIC packets ([`crate::h3::datagram`]) and is
//! delivered — or lost — as a unit. The transport therefore exposes
//! message-oriented [`DatagramTransport::send`] / [`DatagramTransport::recv`]
//! rather than the stream-oriented `read` / `write` of the H/2 socket. Reassembly,
//! retransmission, and ordering are the connection layer's job (loss detection in
//! [`crate::h3::loss`], reassembly in [`crate::h3::stream`]); the transport only
//! moves opaque datagrams to and from the connected peer.
//!
//! ## Blocking model + the timer
//!
//! The connection is driven by a single-threaded event loop that alternates
//! between "wait for the next inbound datagram" and "fire whichever QUIC timer
//! deadline elapsed" ([`crate::h3::timer::ConnectionTimers`]). To let one
//! blocking `recv` call double as the timer wait, the transport supports a read
//! timeout: [`DatagramTransport::set_read_timeout`] arms it, and
//! [`recv_timeout`] converts the timer's next [`Instant`] deadline into the
//! [`Duration`] the event loop should block for. When `recv` returns
//! [`std::io::ErrorKind::WouldBlock`] / [`std::io::ErrorKind::TimedOut`], no
//! datagram arrived before the deadline and the loop fires its timers instead.

use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// A connected, message-oriented transport for QUIC datagrams (RFC 9000 §5).
///
/// The QUIC connection layer is generic over this trait so it can run over a
/// real UDP socket ([`UdpDatagram`]) or a scripted mock
/// ([`MockDatagramTransport`]). Implementations are *connected*: `send` targets
/// a single fixed peer and `recv` only yields datagrams from that peer, so the
/// peer address never appears in the method signatures (it is fixed at
/// construction and reported by [`DatagramTransport::peer_addr`]).
///
/// All methods take `&mut self`: the transport is owned exclusively by its
/// connection and is not shared across threads.
pub trait DatagramTransport {
    /// Send one QUIC datagram to the connected peer as a single UDP payload.
    ///
    /// A datagram is sent atomically: a short write is reported as an error
    /// rather than a partial send, because a truncated QUIC datagram is
    /// unusable. `datagram` is the fully assembled, coalesced-and-encrypted
    /// byte string produced by [`crate::h3::datagram_build`].
    fn send(&mut self, datagram: &[u8]) -> io::Result<()>;

    /// Receive the next datagram from the connected peer into `buf`, returning
    /// the number of bytes written.
    ///
    /// Blocks until a datagram arrives or, if a read timeout is armed
    /// ([`DatagramTransport::set_read_timeout`]), until the timeout elapses — in
    /// which case the error kind is [`io::ErrorKind::WouldBlock`] or
    /// [`io::ErrorKind::TimedOut`] (both are portable spellings of the same
    /// condition; use [`recv_timed_out`] to test for either). A datagram larger
    /// than `buf` is truncated to `buf.len()`, matching UDP `recvfrom`
    /// semantics; callers size `buf` to the maximum QUIC datagram
    /// ([`MAX_DATAGRAM_SIZE`]).
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Arm or disarm the receive timeout.
    ///
    /// `None` makes [`DatagramTransport::recv`] block until a datagram arrives.
    /// `Some(d)` makes it block at most `d`; a `d` of [`Duration::ZERO`] means a
    /// non-blocking poll that returns [`io::ErrorKind::WouldBlock`] immediately
    /// when no datagram is queued. Use [`recv_timeout`] to derive `d` from the
    /// connection timer's next deadline.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;

    /// The local address the transport is bound to (RFC 9000 §9 path identity —
    /// the four-tuple's local half, used by path validation and migration).
    fn local_addr(&self) -> io::Result<SocketAddr>;

    /// The connected peer address (the four-tuple's remote half).
    fn peer_addr(&self) -> io::Result<SocketAddr>;
}

/// The maximum size of a QUIC datagram receive buffer.
///
/// QUIC permits datagrams up to 65527 bytes (RFC 9000 §14, bounded by the
/// `max_udp_payload_size` transport parameter, [`crate::h3::transport_params`]),
/// but the initial anti-amplification / PMTU floor is 1200 bytes
/// ([`crate::h3::path_mtu::QUIC_MIN_PLPMTU`]). A receive buffer sized to the
/// theoretical maximum never truncates an inbound datagram regardless of the
/// negotiated PMTU.
pub const MAX_DATAGRAM_SIZE: usize = 65527;

/// Returns `true` if `err` is the "no datagram arrived before the read timeout"
/// condition.
///
/// A blocking socket with a read timeout reports the deadline differently across
/// platforms — [`io::ErrorKind::WouldBlock`] on Unix, [`io::ErrorKind::TimedOut`]
/// on Windows — so the QUIC event loop tests for either to decide it should fire
/// timers instead of processing a datagram.
pub fn recv_timed_out(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Converts the connection timer's next deadline into the receive timeout the
/// event loop should pass to [`DatagramTransport::set_read_timeout`].
///
/// - `None` (no timer armed) → `None`: block in `recv` until a datagram arrives.
/// - a `deadline` still in the future → `Some(deadline - now)`: block at most
///   that long, so `recv` wakes in time to fire the timer.
/// - a `deadline` at or before `now` → `Some(Duration::ZERO)`: the timer is
///   already due, so poll `recv` without blocking and fire the timer this
///   iteration.
pub fn recv_timeout(deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    deadline.map(|d| d.saturating_duration_since(now))
}

/// A [`DatagramTransport`] backed by a connected [`std::net::UdpSocket`].
///
/// The socket is connected to a single peer at construction, so the kernel
/// filters inbound datagrams to that peer and `send`/`recv` need no address
/// argument. Read-timeout handling maps [`Duration::ZERO`] to non-blocking mode
/// (a `std` socket rejects a zero read timeout), any other duration to the
/// socket read timeout, and `None` back to indefinite blocking.
#[derive(Debug)]
pub struct UdpDatagram {
    socket: UdpSocket,
    /// Whether the socket is currently in non-blocking mode, tracked so a
    /// `Some(ZERO)` → `Some(d)` / `None` transition can restore blocking mode.
    nonblocking: bool,
}

impl UdpDatagram {
    /// Binds a UDP socket to `local` and connects it to `peer`.
    ///
    /// Binding to a port of `0` lets the OS choose an ephemeral source port
    /// (the usual client case); [`DatagramTransport::local_addr`] then reports
    /// the assigned port. Connecting fixes the peer so subsequent
    /// `send`/`recv` are address-free and the kernel drops datagrams from other
    /// sources.
    pub fn connect(local: SocketAddr, peer: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        socket.connect(peer)?;
        Ok(Self {
            socket,
            nonblocking: false,
        })
    }

    /// Wraps an already-connected [`UdpSocket`].
    ///
    /// The socket must already be `connect`ed to a peer; this is the seam for
    /// callers that obtain the socket elsewhere (e.g. a proxy or a
    /// pre-configured source port). Blocking mode is assumed and reset.
    pub fn from_socket(socket: UdpSocket) -> io::Result<Self> {
        socket.set_nonblocking(false)?;
        Ok(Self {
            socket,
            nonblocking: false,
        })
    }

    /// The underlying socket, for callers needing platform-specific tuning.
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl DatagramTransport for UdpDatagram {
    fn send(&mut self, datagram: &[u8]) -> io::Result<()> {
        let n = self.socket.send(datagram)?;
        if n != datagram.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "QUIC datagram truncated on send",
            ));
        }
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.recv(buf)
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match timeout {
            // A `std` socket rejects a zero read timeout; a zero deadline means
            // "poll without blocking", which is non-blocking mode.
            Some(d) if d.is_zero() => {
                if !self.nonblocking {
                    self.socket.set_nonblocking(true)?;
                    self.nonblocking = true;
                }
                // Clear any prior positive read timeout so it does not linger.
                self.socket.set_read_timeout(None)
            }
            other => {
                if self.nonblocking {
                    self.socket.set_nonblocking(false)?;
                    self.nonblocking = false;
                }
                self.socket.set_read_timeout(other)
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.socket.peer_addr()
    }
}

/// A scripted [`DatagramTransport`] for deterministic tests.
///
/// Inbound datagrams are queued in advance and returned by `recv` in order;
/// outbound datagrams are captured in [`MockDatagramTransport::sent`] for the
/// test to assert on. When the inbound queue is empty, `recv` reports
/// [`io::ErrorKind::WouldBlock`] — the same signal a real socket gives when its
/// read timeout elapses — so a connection driver exercises its timer path
/// without any real clock or socket.
#[derive(Debug)]
pub struct MockDatagramTransport {
    inbound: VecDeque<Vec<u8>>,
    /// Every datagram passed to [`DatagramTransport::send`], in order, for the
    /// test to inspect.
    pub sent: Vec<Vec<u8>>,
    local: SocketAddr,
    peer: SocketAddr,
    read_timeout: Option<Duration>,
}

impl MockDatagramTransport {
    /// Creates a mock connected between `local` and `peer` with no inbound
    /// datagrams queued.
    pub fn new(local: SocketAddr, peer: SocketAddr) -> Self {
        Self {
            inbound: VecDeque::new(),
            sent: Vec::new(),
            local,
            peer,
            read_timeout: None,
        }
    }

    /// Queues a datagram for a later [`DatagramTransport::recv`] to return.
    /// Datagrams are returned in the order they are pushed.
    pub fn push_inbound(&mut self, datagram: impl Into<Vec<u8>>) {
        self.inbound.push_back(datagram.into());
    }

    /// The number of inbound datagrams still queued.
    pub fn inbound_len(&self) -> usize {
        self.inbound.len()
    }

    /// The read timeout most recently set via
    /// [`DatagramTransport::set_read_timeout`], for tests asserting the event
    /// loop armed the timer correctly.
    pub fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }
}

impl DatagramTransport for MockDatagramTransport {
    fn send(&mut self, datagram: &[u8]) -> io::Result<()> {
        self.sent.push(datagram.to_vec());
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inbound.pop_front() {
            Some(datagram) => {
                let n = datagram.len().min(buf.len());
                buf[..n].copy_from_slice(&datagram[..n]);
                Ok(n)
            }
            None => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "mock datagram transport: no inbound datagram queued",
            )),
        }
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.read_timeout = timeout;
        Ok(())
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    // ---- recv_timeout ----------------------------------------------------

    #[test]
    fn recv_timeout_none_deadline_blocks_indefinitely() {
        let now = Instant::now();
        assert_eq!(recv_timeout(None, now), None);
    }

    #[test]
    fn recv_timeout_future_deadline_is_remaining() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(250);
        assert_eq!(recv_timeout(Some(deadline), now), Some(Duration::from_millis(250)));
    }

    #[test]
    fn recv_timeout_elapsed_deadline_is_zero() {
        let now = Instant::now();
        let past = now - Duration::from_millis(10);
        assert_eq!(recv_timeout(Some(past), now), Some(Duration::ZERO));
    }

    #[test]
    fn recv_timeout_deadline_at_now_is_zero() {
        let now = Instant::now();
        assert_eq!(recv_timeout(Some(now), now), Some(Duration::ZERO));
    }

    // ---- recv_timed_out --------------------------------------------------

    #[test]
    fn recv_timed_out_matches_wouldblock_and_timedout() {
        assert!(recv_timed_out(&io::Error::from(io::ErrorKind::WouldBlock)));
        assert!(recv_timed_out(&io::Error::from(io::ErrorKind::TimedOut)));
        assert!(!recv_timed_out(&io::Error::from(io::ErrorKind::ConnectionRefused)));
    }

    // ---- MockDatagramTransport ------------------------------------------

    #[test]
    fn mock_send_captures_datagrams_in_order() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        t.send(b"first").unwrap();
        t.send(b"second").unwrap();
        assert_eq!(t.sent, vec![b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn mock_recv_returns_inbound_in_order() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        t.push_inbound(b"alpha".to_vec());
        t.push_inbound(b"beta".to_vec());
        assert_eq!(t.inbound_len(), 2);

        let mut buf = [0u8; MAX_DATAGRAM_SIZE];
        let n = t.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"alpha");
        let n = t.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"beta");
        assert_eq!(t.inbound_len(), 0);
    }

    #[test]
    fn mock_recv_empty_reports_wouldblock() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        let mut buf = [0u8; 64];
        let err = t.recv(&mut buf).unwrap_err();
        assert!(recv_timed_out(&err), "empty mock recv must look like a timeout");
    }

    #[test]
    fn mock_recv_truncates_to_buffer() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        t.push_inbound(vec![0xAB; 100]);
        let mut buf = [0u8; 10];
        let n = t.recv(&mut buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(buf, [0xAB; 10]);
    }

    #[test]
    fn mock_reports_configured_addrs() {
        let t = MockDatagramTransport::new(loopback(5555), loopback(443));
        assert_eq!(t.local_addr().unwrap(), loopback(5555));
        assert_eq!(t.peer_addr().unwrap(), loopback(443));
    }

    #[test]
    fn mock_set_read_timeout_roundtrips() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        assert_eq!(t.read_timeout(), None);
        t.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        assert_eq!(t.read_timeout(), Some(Duration::from_millis(50)));
        t.set_read_timeout(None).unwrap();
        assert_eq!(t.read_timeout(), None);
    }

    // ---- object safety ---------------------------------------------------

    #[test]
    fn transport_is_object_safe() {
        let mut t = MockDatagramTransport::new(loopback(1), loopback(2));
        t.push_inbound(b"dyn".to_vec());
        let dynamic: &mut dyn DatagramTransport = &mut t;
        dynamic.send(b"via-dyn").unwrap();
        let mut buf = [0u8; 16];
        let n = dynamic.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"dyn");
    }

    // ---- UdpDatagram over loopback --------------------------------------
    //
    // Two connected loopback sockets exercise the real socket adapter. UDP on
    // the loopback interface delivers a sent datagram synchronously enough that
    // an immediately following blocking `recv` observes it; the timeout tests
    // never send, so their outcome is timing-independent (no data can arrive).

    /// Connects a pair of loopback `UdpDatagram`s to each other.
    fn connected_pair() -> (UdpDatagram, UdpDatagram) {
        // Bind both to ephemeral ports first to learn the assigned addresses,
        // then connect each to the other.
        let a = UdpSocket::bind(loopback(0)).unwrap();
        let b = UdpSocket::bind(loopback(0)).unwrap();
        let a_addr = a.local_addr().unwrap();
        let b_addr = b.local_addr().unwrap();
        a.connect(b_addr).unwrap();
        b.connect(a_addr).unwrap();
        (
            UdpDatagram::from_socket(a).unwrap(),
            UdpDatagram::from_socket(b).unwrap(),
        )
    }

    #[test]
    fn udp_round_trip() {
        let (mut a, mut b) = connected_pair();
        a.send(b"quic-datagram").unwrap();
        let mut buf = [0u8; MAX_DATAGRAM_SIZE];
        let n = b.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"quic-datagram");
    }

    #[test]
    fn udp_reports_connected_addrs() {
        let (a, b) = connected_pair();
        // a's peer is b's local, and vice versa.
        assert_eq!(a.peer_addr().unwrap(), b.local_addr().unwrap());
        assert_eq!(b.peer_addr().unwrap(), a.local_addr().unwrap());
    }

    #[test]
    fn udp_read_timeout_elapses_to_would_block() {
        let (_a, mut b) = connected_pair();
        // Nothing is ever sent to `b`, so a short read timeout must elapse.
        b.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let mut buf = [0u8; 64];
        let err = b.recv(&mut buf).unwrap_err();
        assert!(recv_timed_out(&err), "unexpected error kind: {:?}", err.kind());
    }

    #[test]
    fn udp_zero_timeout_is_nonblocking() {
        let (_a, mut b) = connected_pair();
        // A zero deadline means poll without blocking; with no datagram queued
        // that is an immediate WouldBlock.
        b.set_read_timeout(Some(Duration::ZERO)).unwrap();
        let mut buf = [0u8; 64];
        let err = b.recv(&mut buf).unwrap_err();
        assert!(recv_timed_out(&err), "unexpected error kind: {:?}", err.kind());
    }

    #[test]
    fn udp_timeout_then_blocking_restores_and_delivers() {
        let (mut a, mut b) = connected_pair();
        // Non-blocking poll first (empty → WouldBlock), then restore blocking
        // mode and confirm a subsequently sent datagram is delivered.
        b.set_read_timeout(Some(Duration::ZERO)).unwrap();
        let mut buf = [0u8; MAX_DATAGRAM_SIZE];
        assert!(b.recv(&mut buf).is_err());

        b.set_read_timeout(None).unwrap();
        a.send(b"after-restore").unwrap();
        let n = b.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"after-restore");
    }
}
