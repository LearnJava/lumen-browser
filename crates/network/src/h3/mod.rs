//! HTTP/3 client implementation (RFC 9114) over QUIC (RFC 9000).
//!
//! Slice breakdown (mirrors the HTTP/2 sprint 5A layering — pure codecs first,
//! IO/connection state later):
//!
//! - Slice 1 — QUIC variable-length integer codec ([`varint`], RFC 9000 §16)
//!   and the HTTP/3 frame codec ([`frame`], RFC 9114 §7.2). Pure parse/
//!   serialize, no IO, no connection state.
//! - Slice 2 — QPACK field-section codec ([`qpack`], RFC 9204), static table
//!   only (the wire behaviour of a peer advertising a zero-size dynamic
//!   table). Pure encode/decode of the header block carried in HEADERS /
//!   PUSH_PROMISE frames; no dynamic table, no encoder/decoder streams.
//! - Slice 3 — the `Alt-Svc` discovery layer ([`alt_svc`], RFC 7838): parses
//!   the response header that advertises HTTP/3 for an origin and caches the
//!   `h3` alternatives per origin with TTL expiry. Pure parse + in-memory
//!   cache, no IO on the parse path (only the `*_now` cache wrappers read the
//!   clock). This is the trigger that later routes a request onto QUIC.
//! - Slice 4 — the QUIC transport frame codec ([`quic_frame`], RFC 9000 §19):
//!   pure parse/serialize of every QUIC frame type (PADDING…HANDSHAKE_DONE)
//!   the connection layer exchanges inside a packet's protected payload, on
//!   the same [`varint`] primitive as the HTTP/3 frame codec. No packet
//!   protection, no packet-number spaces, no IO.
//! - Slice 5 — the QUIC packet header codec ([`packet`], RFC 9000 §17): pure
//!   parse/serialize of every packet shape (Initial, 0-RTT, Handshake, Retry,
//!   Version Negotiation, and the short 1-RTT header), carrying the
//!   header-protected first-byte bits and the AEAD-protected payload verbatim
//!   as opaque bytes. No header protection, no packet protection, no IO. This
//!   is the frame the connection layer parses first, before removing header
//!   protection and AEAD-decrypting the payload into [`quic_frame`] frames.
//! - Slice 6 — the QPACK dynamic table + instruction streams ([`qpack_stream`],
//!   RFC 9204 §3.2, §4.3, §4.4): the shared dynamic table (byte-budget
//!   capacity, FIFO eviction, absolute/relative indexing) plus the encoder
//!   stream (Set Capacity / Insert With Name Reference / Insert With Literal
//!   Name / Duplicate) and the decoder stream (Section Acknowledgment / Stream
//!   Cancellation / Insert Count Increment). Pure parse/serialize plus the
//!   in-memory table; applying an encoder stream reproduces the peer's table
//!   state. No IO, no unidirectional-stream framing.
//! - Slice 7 — the QUIC RTT estimator + NewReno congestion controller
//!   ([`recovery`], RFC 9002 §5, §7): pure state machines the loss-recovery
//!   layer drives with acked/lost packets. The estimator produces the smoothed
//!   RTT and probe timeout (RFC 9002 §6.2.1); the controller tracks the
//!   congestion window through slow start, congestion avoidance, and recovery,
//!   halving it on loss (RFC 9002 §7.3.2) and collapsing it under persistent
//!   congestion (RFC 9002 §7.6). No sent-packet registry, no loss detection, no
//!   IO — that is the next slice.
//! - Slice 8 — the QUIC sent-packet registry + loss detection ([`loss`],
//!   RFC 9002 §6): the per-packet-number-space registry of in-flight packets,
//!   ack processing that removes newly-acknowledged packets and produces the RTT
//!   sample, and the packet-threshold (§6.1.1) and time-threshold (§6.1.2) loss
//!   detection that decides which packets are lost and feeds [`recovery`]. Pure
//!   state machine driven by decoded ACK frames and a caller-supplied clock; no
//!   PTO timer, no IO.
//! - Slice 9 — the QUIC loss-detection timer + PTO ([`pto`], RFC 9002 §6.2,
//!   Appendix A): [`pto::LossDetection`] ties the three per-space registries and
//!   the RTT estimator into the single loss-detection timer. It computes
//!   `SetLossDetectionTimer` (earliest time-threshold loss time, else the
//!   exponentially-backed-off probe timeout, else disarm) and drives
//!   `OnLossDetectionTimeout` (declare time-threshold losses, or send one/two
//!   ack-eliciting probes and bump the backoff), including the anti-deadlock PTO
//!   and the Application-Data-until-handshake-confirmed guard. Pure state machine
//!   driven by a caller-supplied clock; no timer IO, no probe assembly.
//! - Slice 10 — the QUIC stream data model ([`stream`], RFC 9000 §2, §3, §4):
//!   the per-stream receive reassembly buffer, per-stream flow-control
//!   accounting on both directions (RFC 9000 §4.1), the final-size invariants
//!   (RFC 9000 §4.5), and the send/receive stream state machines (RFC 9000 §3).
//!   [`stream::RecvStream`] merges out-of-order / overlapping STREAM frames into
//!   an ordered byte stream and re-advertises the receive window; [`stream::
//!   SendStream`] buffers application data and emits STREAM frames bounded by the
//!   peer's flow-control limit, advancing to `DataRecvd` on acknowledgement. Pure
//!   state machine driven by decoded frames; no connection-level flow control, no
//!   retransmission, no IO.
//! - Slice 11 — the connection-level flow control + stream-count limits
//!   ([`conn_flow`], RFC 9000 §4.1, §4.6): the connection-wide `MAX_DATA` budget
//!   that caps the sum of stream data across all streams (independent of each
//!   stream's own `MAX_STREAM_DATA`) and the `MAX_STREAMS` budget that caps how
//!   many streams of each direction an endpoint may open. [`conn_flow::
//!   SendConnFlow`] / [`conn_flow::RecvConnFlow`] track the send/receive halves of
//!   the connection data budget; [`conn_flow::SendStreamLimit`] / [`conn_flow::
//!   RecvStreamLimit`] track the send/receive halves of the stream-count budget,
//!   including the block signals (`DATA_BLOCKED` / `STREAMS_BLOCKED`) and the
//!   re-advertisement as data is consumed and streams complete. Pure state
//!   machine driven by the connection layer's sent/received/opened/closed
//!   reports; no IO.
//! - Slice 12 — the HTTP/3 stream layer ([`h3_stream`], RFC 9114 §6.2, §7.1,
//!   §4.1): unidirectional stream-type demux ([`h3_stream::UniStreamType`] —
//!   control / push+Push-ID / QPACK encoder / QPACK decoder / reserved), the
//!   "exactly one control / QPACK-encoder / QPACK-decoder stream" rule
//!   ([`h3_stream::UniStreamRegistry`], `H3_STREAM_CREATION_ERROR` on a duplicate,
//!   `H3_CLOSED_CRITICAL_STREAM` on closing one), the control-stream frame grammar
//!   ([`h3_stream::ControlStream`] — first frame is SETTINGS else
//!   `H3_MISSING_SETTINGS`, SETTINGS at most once, no request frames), and the
//!   request/response-stream frame grammar ([`h3_stream::RequestStream`] —
//!   HEADERS+ → DATA* → optional trailer HEADERS, interleaved PUSH_PROMISE,
//!   everything else `H3_FRAME_UNEXPECTED`). Pure state machine over decoded
//!   [`frame::Frame`]s; no IO. Reuses [`frame`]'s type codes and
//!   `H3_FRAME_UNEXPECTED`.
//! - Slice 13 — the QUIC key schedule ([`key_schedule`], RFC 9001 §5.1, §5.2):
//!   the TLS 1.3 HKDF (`HKDF-Extract` / `HKDF-Expand` / `HKDF-Expand-Label`,
//!   RFC 5869 + RFC 8446 §7.1) built on the existing SHA-256 dependency, the
//!   QUIC v1 Initial salt, and the Initial-secret chain that derives both
//!   directions' packet-protection keys (`key` / `iv` / `hp`, labels
//!   `"quic key"` / `"quic iv"` / `"quic hp"`) deterministically from the
//!   client's original Destination Connection ID, plus the `"quic ku"` key
//!   update (§6.1). Pure functions; validated against the RFC 9001 Appendix A.1
//!   test vectors. The header-protection and AEAD transforms that consume this
//!   material are the next slices.
//! - Slice 14 — QUIC packet protection ([`packet_protect`], RFC 9001 §5.3, §5.4):
//!   the two transforms that consume slice 13's key material. AEAD payload
//!   protection seals/opens the packet payload with AES-128-GCM
//!   ([`packet_protect::aes_128_gcm_seal`] / [`packet_protect::aes_128_gcm_open`]),
//!   deriving the nonce from the `iv` and packet number and authenticating the
//!   unprotected header as associated data. Header protection masks the first
//!   byte's low bits and the packet-number octets with a mask derived from an
//!   AES-ECB sample of the ciphertext ([`packet_protect::aes_128_hp_mask`],
//!   [`packet_protect::apply_header_protection`] /
//!   [`packet_protect::remove_header_protection`]). AES suite only (ChaCha20
//!   deferred). Pure functions over byte buffers; validated against the RFC 9001
//!   Appendix A.2/A.3 vectors.
//! - Slice 15 — the TLS 1.3 key schedule ([`tls_schedule`], RFC 8446 §7.1) that
//!   produces the Handshake and 1-RTT (application) traffic secrets QUIC uses at
//!   the encryption levels TLS negotiates (RFC 9001 §5), on the same HKDF
//!   primitives as slice 13. `Transcript-Hash` / `Derive-Secret`
//!   ([`tls_schedule::transcript_hash`], [`tls_schedule::derive_secret`]) over
//!   SHA-256, the no-PSK Early Secret ([`tls_schedule::early_secret`]), the
//!   `(EC)DHE`-mixed Handshake Secret ([`tls_schedule::handshake_secret`]) and
//!   its per-direction traffic secrets ([`tls_schedule::HandshakeTrafficSecrets`]),
//!   the Master Secret ([`tls_schedule::master_secret`]) and its 1-RTT traffic
//!   secrets ([`tls_schedule::ApplicationTrafficSecrets`]), plus the exporter /
//!   resumption secrets. Each traffic secret bridges to QUIC keys through the
//!   existing [`key_schedule::PacketProtectionKeys`]. Pure functions; validated
//!   against the RFC 8448 §3 handshake trace. The X25519 agreement and the
//!   handshake message codecs that feed the `(EC)DHE` secret and transcript
//!   hashes are later slices.
//! - Slice 16 — the TLS 1.3 handshake message codec ([`tls_message`],
//!   RFC 8446 §4): the [`tls_message::Handshake`] wrapper (`msg_type` ·
//!   `uint24 length` · body) and every message a QUIC client sends or receives
//!   (ClientHello, ServerHello/HelloRetryRequest, EncryptedExtensions,
//!   CertificateRequest, Certificate, CertificateVerify, Finished,
//!   NewSessionTicket, KeyUpdate, EndOfEarlyData), with extensions carried
//!   generically as [`tls_message::Extension`] so the transcript stays
//!   byte-exact. Typed codecs for the two extension bodies the QUIC/TLS bridge
//!   needs — [`tls_message::KeyShareEntry`] (the `(EC)DHE` public value that
//!   feeds slice 15's [`tls_schedule::handshake_secret`]) and
//!   [`tls_message::supported_versions`]. Pure parse/serialize; the produced
//!   bytes are exactly the `Transcript-Hash` input of slice 15. Validated by
//!   byte-exact round-trip of the RFC 8448 §3 ServerHello. Out of scope: the
//!   X25519 agreement over two key shares, and computing/verifying the
//!   `Finished` MAC and `CertificateVerify` signature.
//! - Slice 17 — the X25519 key agreement ([`key_agreement`], RFC 7748,
//!   RFC 8446 §4.2.8) that turns two [`tls_message::KeyShareEntry`] public keys
//!   into the raw `(EC)DHE` shared secret [`tls_schedule::handshake_secret`]
//!   consumes. [`key_agreement::x25519_public_key`] derives our ephemeral public
//!   value for the `key_share` extension; [`key_agreement::x25519_shared_secret`]
//!   / [`key_agreement::x25519_ecdhe_from_key_share`] perform the Curve25519
//!   scalar multiplication against the peer's share (rejecting a small-order,
//!   non-contributory key per RFC 7748 §6.1). The X25519 primitive comes from
//!   `x25519-dalek` (constant-time); the module core is deterministic and
//!   validated against the RFC 7748 §5.2/§6.1 test vectors, with only the
//!   optional [`key_agreement::generate_x25519_private_key`] reading OS entropy.
//! - Slice 18 — the TLS 1.3 `Finished` MAC ([`tls_finished`], RFC 8446 §4.4.4):
//!   the handshake key-confirmation that binds the whole transcript, carried in
//!   QUIC CRYPTO frames (RFC 9001 §4). [`tls_finished::finished_key`] derives the
//!   per-direction `finished_key = HKDF-Expand-Label(BaseKey, "finished", "",
//!   Hash.length)` from a sender's handshake traffic secret ([`tls_schedule::
//!   HandshakeTrafficSecrets`]); [`tls_finished::finished_verify_data`] is the
//!   `verify_data = HMAC(finished_key, Transcript-Hash(…))` a sender writes; and
//!   [`tls_finished::verify_finished`] is the constant-time check a receiver runs
//!   against a peer's `Finished` (a mismatch is a fatal `decrypt_error`). Pure
//!   functions over SHA-256, reusing [`key_schedule`]'s HMAC and
//!   `HKDF-Expand-Label`; the transcript hash comes from [`tls_message`]. No new
//!   dependency, no IO. Validated end-to-end against the RFC 8448 §3 server /
//!   client `Finished` values. Out of scope: the `CertificateVerify` signature
//!   (RFC 8446 §4.4.3, a public-key verification over the same transcript).
//! - Slice 19 — the TLS 1.3 `CertificateVerify` signature ([`tls_cert_verify`],
//!   RFC 8446 §4.4.3): the peer-authentication step of the handshake. QUIC
//!   carries this message in CRYPTO frames (RFC 9001 §4), and unlike the
//!   `Finished` MAC (slice 18, which only confirms key agreement) it proves the
//!   peer holds the private key of the end-entity certificate.
//!   [`tls_cert_verify::certificate_verify_content`] builds the signed content
//!   (64 `0x20` octets · role context string · `0x00` · transcript hash, so a
//!   TLS 1.3 signature cannot be confused across versions or roles);
//!   [`tls_cert_verify::verify_certificate_verify`] verifies the DER signature
//!   under the peer's public key for the named [`tls_cert_verify::signature_scheme`].
//!   Only `ecdsa_secp256r1_sha256` (an RFC 8446 §9.1 mandatory-to-implement
//!   scheme) is wired, reusing the existing `p256` dependency (WebAuthn ES256) —
//!   no new dependency. Pure functions; the ECDSA primitive is validated against
//!   the RFC 6979 Appendix A.2.5 P-256/SHA-256 vector. Out of scope:
//!   `rsa_pss_rsae_sha256` / `ed25519` verifiers and X.509 `SubjectPublicKeyInfo`
//!   extraction (the caller passes the SEC1 EC point).
//! - Slice 20 — the `ed25519` `CertificateVerify` scheme
//!   ([`tls_cert_verify::ed25519_verify`], RFC 8446 §4.2.3, RFC 8032): EdDSA over
//!   Curve25519, which (unlike the ECDSA schemes) signs the signed content
//!   directly with no prehash and carries the signature raw with no DER wrapper.
//!   [`tls_cert_verify::verify_certificate_verify`] now dispatches
//!   `ecdsa_secp256r1_sha256` and `ed25519`; the public key is the raw 32-octet
//!   Ed25519 point (RFC 8410 §4). The verifier comes from `ed25519-dalek`, which
//!   reuses the `curve25519-dalek` already in the tree via `x25519-dalek` (same
//!   dalek family, constant-time). Pure functions; validated against the RFC 8032
//!   §7.1 TEST 1 vector. Out of scope: `rsa_pss_rsae_sha256` and the P-384/P-521
//!   variants (still [`tls_cert_verify::CertVerifyError::UnsupportedScheme`]).
//! - Slice 21 — the `rsa_pss_rsae_sha256` `CertificateVerify` scheme
//!   ([`tls_cert_verify::rsa_pss_sha256_verify`], RFC 8446 §4.2.3, RFC 8017 §8.1):
//!   RSASSA-PSS with the MGF1-SHA-256 mask and SHA-256 message hash — the
//!   signature the great majority of real server certificates carry.
//!   [`tls_cert_verify::verify_certificate_verify`] now dispatches
//!   `ecdsa_secp256r1_sha256`, `ed25519`, and `rsa_pss_rsae_sha256`; the public key
//!   is the PKCS#1 DER `RSAPublicKey` (the `subjectPublicKey` of an rsaEncryption
//!   `SubjectPublicKeyInfo`) and the signature is the raw big-endian integer TLS
//!   carries. The verifier comes from the `rsa` crate (pure-Rust RustCrypto,
//!   reusing the `sha2` already in the tree); the PSS salt length is recovered
//!   during EMSA-PSS-VERIFY, so RFC 8446's salt-equals-digest-length signatures
//!   verify interoperably. Pure functions; validated end-to-end by signing a
//!   `CertificateVerify` content with a generated RSA key and rejecting every
//!   tampering. Out of scope: the P-384/P-521 ECDSA variants and the SHA-384/512
//!   RSA-PSS variants (still [`tls_cert_verify::CertVerifyError::UnsupportedScheme`]).
//! - Slice 22 — the `rsa_pss_rsae_sha384` and `rsa_pss_rsae_sha512`
//!   `CertificateVerify` schemes ([`tls_cert_verify::rsa_pss_sha384_verify`],
//!   [`tls_cert_verify::rsa_pss_sha512_verify`], RFC 8446 §4.2.3, RFC 8017 §8.1):
//!   the SHA-384/512 siblings of slice 21, identical but for the MGF1 and message
//!   digest, commonly signed by 3072/4096-bit certificates. All three RSA-PSS
//!   variants now share one generic verifier over `D: Digest`, reusing the `rsa`
//!   crate and the `sha2` digests already in the tree — no new dependency.
//!   [`tls_cert_verify::verify_certificate_verify`] now dispatches
//!   `ecdsa_secp256r1_sha256`, `ed25519`, and `rsa_pss_rsae_sha256/384/512`. Pure
//!   functions; validated end-to-end by signing a `CertificateVerify` content with
//!   generated RSA keys and rejecting a cross-digest signature. Out of scope: the
//!   P-384/P-521 ECDSA variants (still
//!   [`tls_cert_verify::CertVerifyError::UnsupportedScheme`]).
//! - Slice 24 — QUIC datagram coalescing ([`datagram`], RFC 9000 §12.2, §14.1):
//!   the pure layer over [`packet`] that splits one received UDP datagram into
//!   its ordered sequence of coalesced packets ([`datagram::parse_datagram`]) and
//!   assembles a sequence back into one datagram ([`datagram::encode_datagram`]).
//!   Only the length-delimited long-header forms (Initial / 0-RTT / Handshake)
//!   can be followed by another packet, so a short-header / Retry / Version
//!   Negotiation packet may appear only last — enforced on encode
//!   ([`datagram::DatagramError::UnterminatedCoalescing`]) and automatic on parse
//!   (those forms consume the datagram tail). [`datagram::initial_padding_shortfall`]
//!   reports how far below the [`datagram::MIN_INITIAL_DATAGRAM_LEN`] (RFC 9000
//!   §14.1) a client Initial datagram is, for the frame layer to pad inside the
//!   packet before encryption. Pure functions, no IO.
//! - Slice 25 — the QUIC transport parameters codec ([`transport_params`],
//!   RFC 9000 §18, §7.4): the pure parse/serialize of the
//!   `quic_transport_parameters` TLS extension body (RFC 9001 §8.2) — a bare
//!   sequence of `(id, length, value)` entries over [`varint`] — into a typed
//!   [`transport_params::TransportParameters`], with the RFC 9000 §18.2
//!   validation (single occurrence per id, the `max_udp_payload_size` /
//!   `ack_delay_exponent` / `max_ack_delay` / `active_connection_id_limit`
//!   ranges, the fixed-width `stateless_reset_token` and preferred-address
//!   fields) and default resolution. These are the values that configure the
//!   connection state machines built above — the peer's `initial_max_data`
//!   seeds [`conn_flow::SendConnFlow`], its `initial_max_stream_data_*` bound
//!   [`stream::SendStream`], its `initial_max_streams_*` seed the [`conn_flow`]
//!   stream limits, its `max_udp_payload_size` clamps the [`recovery`] datagram
//!   size, and its `ack_delay_exponent` / `max_ack_delay` scale the ACK-delay
//!   handling in [`loss`] / [`pto`]. Unknown / reserved (GREASE, RFC 9000 §18.1)
//!   ids are preserved verbatim so the round-trip is byte-stable. Pure
//!   functions, no IO; wiring the values into a live connection is a later slice.
//! - Slice 26 — the QUIC packet number encoding/decoding ([`packet_number`],
//!   RFC 9000 §17.1, Appendix A): the truncation codec between the packet header
//!   codec [`packet`] — which carries the packet number inside its opaque
//!   `protected` region and its two-bit Packet Number Length in the
//!   header-protected first byte — and the loss-recovery layer [`loss`] /
//!   [`recovery`], which reasons in full 62-bit packet numbers. On send,
//!   [`packet_number::packet_number_length`] picks the fewest bytes `b ∈ 1..=4`
//!   with `2^(8·b − 1) ≥ num_unacked` (Appendix A.2) and
//!   [`packet_number::encode_packet_number`] appends that many least-significant
//!   big-endian bytes, with [`packet_number::encode_pn_length_bits`] supplying the
//!   header's length field. On receive, [`packet_number::decode_packet_number`]
//!   reconstructs the full number nearest `largest_pn + 1` from the truncation and
//!   its width (Appendix A.3), with [`packet_number::pn_length_from_first_byte`]
//!   and [`packet_number::read_truncated_packet_number`] recovering what the
//!   header codec left opaque. Pure functions; validated against the RFC 9000
//!   Appendix A.2/A.3 examples. No IO — wiring this into the header-protection /
//!   AEAD path ([`packet_protect`]) is the connection layer's job.
//! - Slice 27 — QUIC Path MTU Discovery ([`path_mtu`], RFC 9000 §14.2–14.4,
//!   RFC 8899): the pure DPLPMTUD state machine that finds the largest UDP
//!   payload a path can carry — the *max datagram size* that bounds the
//!   congestion window in [`recovery`] and the size of the packets the sender
//!   builds. [`path_mtu::PathMtuDiscovery`] confirms the
//!   [`path_mtu::QUIC_MIN_PLPMTU`] base size (RFC 8899 §5.2; a completed
//!   handshake already proves it, so
//!   [`path_mtu::PathMtuDiscovery::with_confirmed_base`] skips straight to the
//!   search), then binary-searches upward: [`path_mtu::PathMtuDiscovery::next_probe`]
//!   proposes the next probe size, an acknowledged probe raises the confirmed
//!   size, and a probe lost [`path_mtu::MAX_PROBES`] times lowers the upper bound
//!   (RFC 8899 §5.3). [`path_mtu::PathMtuDiscovery::on_black_hole`] drops back to
//!   the base and restarts when ordinary datagrams at the current size disappear
//!   (RFC 8899 §5.4). Pure state machine driven by probe acknowledgement / loss;
//!   no IO, no probe-packet assembly, and the caller must keep a lost probe out
//!   of the congestion controller (RFC 9000 §14.4).
//! - Slice 28 — QUIC ACK generation ([`ack`], RFC 9000 §13.2, §19.3): the
//!   receiver-side mirror of [`loss`]. [`ack::AckGenerator`] (one per
//!   [`loss::PacketNumberSpace`]) records the packet numbers *we received* as a set
//!   of disjoint inclusive ranges, decides when an acknowledgement is owed —
//!   immediately on a reordered/gap-filling packet, on reaching the ack-eliciting
//!   threshold (RFC 9000 §13.2.2), on an ECN-CE mark, or in the Initial/Handshake
//!   spaces that never delay, else within the peer's `max_ack_delay` — and builds the
//!   [`quic_frame::Frame::Ack`] reporting those ranges largest-first with the scaled
//!   ACK Delay and ECN counts. [`ack::AckGenerator::on_ack_of_ack`] bounds the range
//!   set once the peer acknowledges one of our ACKs (RFC 9000 §13.2.4). Pure state
//!   machine driven by a caller-supplied clock; no IO, no timer arming, no packet
//!   assembly.
//! - Slice 29+ (planned) — the rest of the QUIC transport: the UDP send/receive,
//!   actually arming the PTO/ACK-delay timers and assembling probe datagrams, the
//!   QPACK encoder/decoder stream instruction wiring, and `h3_do_request` dispatch
//!   alongside the existing H1/H2 paths.
//!
//! The codecs here are the shared foundation: QUIC varints delimit both
//! transport-layer fields and HTTP/3 frames, the QUIC frame codec carries the
//! transport payload, the HTTP/3 frame codec carries an opaque QPACK field
//! block, [`qpack`] turns that block into header fields, and [`alt_svc`]
//! decides when an origin is eligible for the QUIC path at all.

pub mod ack;
pub mod alt_svc;
pub mod conn_flow;
pub mod datagram;
pub mod frame;
pub mod h3_stream;
pub mod key_agreement;
pub mod key_schedule;
pub mod loss;
pub mod packet;
pub mod packet_number;
pub mod packet_protect;
pub mod path_mtu;
pub mod pto;
pub mod qpack;
pub mod qpack_stream;
pub mod quic_frame;
pub mod recovery;
pub mod stream;
pub mod tls_cert_verify;
pub mod tls_finished;
pub mod tls_message;
pub mod tls_schedule;
pub mod transport_params;
pub mod varint;
