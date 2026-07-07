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
//! - Slice 29 — the QUIC packet protection pipeline ([`packet_crypt`], RFC 9001
//!   §5.3, §5.4): the single place that ties the header codec [`packet`], the
//!   packet-number truncation codec [`packet_number`], the AEAD + header
//!   protection transforms [`packet_protect`], and the [`key_schedule`] key set
//!   into the two end-to-end operations the connection layer runs on every
//!   1-RTT-bearing packet. [`packet_crypt::encrypt_packet`] assembles the header,
//!   places the clear packet number, AEAD-seals the payload with the unprotected
//!   header as associated data, and applies header protection, returning the
//!   on-wire packet; [`packet_crypt::decrypt_packet`] is the inverse — it parses
//!   the header, removes header protection, reconstructs the full 62-bit packet
//!   number (Appendix A.3), and AEAD-opens the payload, reporting the bytes
//!   consumed so a coalesced datagram (RFC 9000 §12.2) is walked packet by
//!   packet. AES suite only, matching [`packet_protect`]. Pure functions over
//!   byte buffers and a supplied key set; the round-trip and the in-the-clear
//!   header layout are validated against the RFC 9001 Appendix A.2 client Initial.
//! - Slice 31 — the QUIC CRYPTO stream ([`crypto_stream`], RFC 9000 §7.5,
//!   §19.6): the per-encryption-level reassembly of the TLS handshake byte
//!   stream carried in CRYPTO frames ([`quic_frame::Frame::Crypto`], RFC 9001
//!   §4), the bridge between the decoded transport frames and the [`tls_message`]
//!   handshake codec. Unlike [`stream`] it has no stream ID, no flow-control
//!   window, and no FIN. [`crypto_stream::CryptoRecvStream`] merges out-of-order
//!   / overlapping / duplicated CRYPTO frames into the contiguous handshake
//!   prefix, enforcing a reassembly bound
//!   ([`crypto_stream::CryptoStreamError::BufferExceeded`] →
//!   `CRYPTO_BUFFER_EXCEEDED`, RFC 9000 §7.5); [`crypto_stream::CryptoSendStream`]
//!   buffers the handshake bytes TLS emits, hands them back as CRYPTO frames
//!   bounded by a size cap, and tracks the acknowledged ranges. Pure state
//!   machine driven by decoded frames; no retransmission, no IO.
//! - Slice 32 — QUIC connection-ID management ([`conn_id`], RFC 9000 §5.1): the
//!   two connection-ID sets driven by the NEW_CONNECTION_ID (RFC 9000 §19.15) and
//!   RETIRE_CONNECTION_ID (RFC 9000 §19.16) frames [`quic_frame`] already codecs.
//!   [`conn_id::RemoteConnIds`] holds the IDs the peer issues for us to stamp on
//!   the packets we send: seeded with the peer's handshake Source Connection ID
//!   (sequence 0), it folds in each NEW_CONNECTION_ID frame — rejecting a
//!   `Retire Prior To` past the sequence number or an out-of-range ID
//!   ([`conn_id::ConnIdError::Malformed`] → `FRAME_ENCODING_ERROR`), a sequence
//!   number reused for a different ID/token
//!   ([`conn_id::ConnIdError::SequenceConflict`] → `PROTOCOL_VIOLATION`), and an
//!   active-set overflow past the advertised `active_connection_id_limit`
//!   ([`conn_id::ConnIdError::LimitExceeded`] → `CONNECTION_ID_LIMIT_ERROR`) —
//!   honouring `Retire Prior To` (RFC 9000 §19.15) and reporting the sequence
//!   numbers the connection layer must RETIRE_CONNECTION_ID, plus the
//!   stateless-reset-token match (RFC 9000 §10.3.1) and voluntary migration.
//!   [`conn_id::LocalConnIds`] holds the IDs we issue for the peer, assigning
//!   monotonic sequence numbers, refusing to exceed the peer's limit, and dropping
//!   an ID on RETIRE_CONNECTION_ID (a retire of an unissued ID is a
//!   `PROTOCOL_VIOLATION`). Pure state machines driven by decoded frames; no IO,
//!   no packet-level Destination-CID context.
//! - Slice 33 — QUIC Retry packet integrity + token handling ([`retry`],
//!   RFC 9000 §17.2.5, §8.1; RFC 9001 §5.8): the pure crypto + state a client
//!   runs on a received Retry packet ([`packet::Packet::Retry`]). A stateless
//!   server answers the client's first Initial with a Retry that carries an
//!   address-validation Token to echo, a fresh Source Connection ID the client
//!   adopts as its new Destination CID, and a 16-byte Retry Integrity Tag.
//!   [`retry::retry_integrity_tag`] / [`retry::verify_retry_integrity`] compute
//!   and check that tag — `AEAD_AES_128_GCM` over an empty plaintext with the
//!   version-fixed [`retry::RETRY_KEY_V1`] / [`retry::RETRY_NONCE_V1`] (reusing
//!   [`packet_protect::aes_128_gcm_seal`]), authenticating the *Retry
//!   Pseudo-Packet*: the Original Destination CID the client chose for its first
//!   Initial (length-prefixed) followed by the Retry packet up to the tag. Since
//!   the ODCID is never on the wire, an off-path attacker cannot forge a Retry.
//!   [`retry::RetryHandler`] is the client state machine: it verifies the tag,
//!   enforces at-most-one-Retry-per-connection (RFC 9000 §17.2.5), and reports
//!   the [`retry::RetryOutcome`] (new DCID + Token). Pure functions and state,
//!   no IO; validated against the RFC 9001 Appendix A.4 vector.
//! - Slice 34 — QUIC packet payload assembly ([`packet_payload`], RFC 9000
//!   §12.4, §13.2.1, §14.1; RFC 9002 §2): the layer between [`quic_frame`] and
//!   [`packet_crypt`] that decides which frames go into a packet and packs them
//!   to a byte budget. [`packet_payload::PacketType`] names the four frame-bearing
//!   packet types and answers the §12.4 permission table
//!   ([`packet_payload::PacketType::permits`]) — finer than the three
//!   [`loss::PacketNumberSpace`] values, since 0-RTT and 1-RTT share the
//!   Application Data space yet admit different frames (ACK, CRYPTO,
//!   HANDSHAKE_DONE, NEW_TOKEN). [`packet_payload::PayloadBuilder`] accumulates
//!   permitted frames up to a limit (rejecting a frame that does not fit without
//!   mutating the payload, and a frame not permitted in the type as a
//!   [`packet_payload::PayloadError::FrameNotPermitted`]), tracks whether the
//!   packet is ack-eliciting (RFC 9000 §13.2.1) and in flight (RFC 9002 §2), and
//!   pads to a target size for the 1200-byte client Initial datagram (RFC 9000
//!   §14.1) or a [`path_mtu`] probe. Pure state over byte buffers; packet-number
//!   assignment, header framing, and encryption remain the caller's job.
//! - Slice 35 — the QUIC connection lifecycle ([`lifecycle`], RFC 9000 §10): the
//!   pure state machine of the active/closing/draining/closed transitions.
//!   [`lifecycle::ConnectionLifecycle`] tracks the idle-timeout deadline
//!   (§10.1 — negotiating the effective `max_idle_timeout` from both endpoints'
//!   advertised values, restarting the timer on a received packet or the first
//!   ack-eliciting packet sent since, and honouring the `3·PTO` floor), the
//!   immediate close that sends a `CONNECTION_CLOSE` and enters the closing state
//!   (§10.2.1, with exponentially rate-limited resends in answer to stray
//!   packets), the draining state entered on receiving a peer `CONNECTION_CLOSE`
//!   (§10.2.2, which sends nothing further), and the `3·PTO` closing/draining
//!   period after which the state is discarded. Pure state machine driven by a
//!   caller-supplied clock and Probe Timeout ([`recovery::RttEstimator::pto`]);
//!   no timer IO, no key retention.
//! - Slice 36 — QUIC path validation + anti-amplification limit
//!   ([`path_validation`], RFC 9000 §8.1, §8.2): the two mechanisms that guard an
//!   unvalidated peer address. [`path_validation::AntiAmplificationLimit`] caps the
//!   bytes an endpoint may send to at most three times the bytes received from that
//!   address (§8.1), lifting the cap once the address is validated;
//!   [`path_validation::PathValidator`] is the sender-side state machine that emits
//!   a `PATH_CHALLENGE` of eight caller-supplied unpredictable bytes, validates the
//!   path on a matching `PATH_RESPONSE` (§8.2.3, any outstanding challenge on any
//!   path), and abandons validation after `3·PTO` (§8.2.4);
//!   [`path_validation::respond_to_challenge`] is the receiver-side echo (§8.2.2).
//!   Pure state machine driven by a caller-supplied clock, PTO, and challenge
//!   bytes; no randomness, no timer IO, no datagram assembly — the full
//!   connection-migration orchestration (§9) is deferred.
//! - Slice 37 — QUIC connection migration ([`path_migration`], RFC 9000 §9): the
//!   orchestration that lets a connection survive a path change, tying slice 36's
//!   [`path_validation`] primitives to [`conn_id`] and [`recovery`].
//!   [`path_migration::is_probing_frame`] / [`path_migration::is_probing_packet`]
//!   classify probing vs non-probing packets (§9.1). [`path_migration::ConnectionMigration`]
//!   gates migration on a confirmed handshake (§9), starts validating a new path
//!   with a `PATH_CHALLENGE` while recording the fresh peer connection-ID sequence
//!   to stamp on it (§9.2, §9.5), enforces the new path's anti-amplification limit
//!   while the peer address there is unvalidated (§9.3), and on a matching
//!   `PATH_RESPONSE` reports the [`path_migration::MigrationOutcome`] — which peer
//!   connection ID is now active (§9.5) and whether the caller must reset the
//!   congestion controller and RTT estimator, unless the change was port-only
//!   (§9.4). A lapsed validation reverts to the old path. Pure state machine
//!   driven by a caller-supplied clock, PTO, and challenge bytes; the
//!   connection-ID retirement ([`conn_id::RemoteConnIds::switch_to`]) and
//!   congestion reset are the caller's job, and simultaneous multi-path use is out
//!   of scope.
//! - Slice 38 — QUIC client-side version negotiation ([`version_nego`],
//!   RFC 9000 §6.2): the pure state a client runs on a received Version
//!   Negotiation packet ([`packet::Packet::VersionNegotiation`]), mirroring
//!   slice 33's [`retry`] handling of a Retry. [`version_nego::VersionNegotiator`]
//!   holds the client's ordered version preferences and its attempted version,
//!   validates the packet (echoed connection IDs, RFC 9000 §6.1), enforces the
//!   §6.2 rules — process at most one and only before any other packet, discard a
//!   packet that lists the attempted version (forged/erroneous), select the
//!   most-preferred mutually supported version and abandon on an empty
//!   intersection — and reports the [`version_nego::VersionNegotiationOutcome`]
//!   (the version to restart with). Pure state machine, no IO; the caller
//!   re-derives Initial keys and re-sends its Initial. Downgrade protection
//!   proper (RFC 9000 §6.3) lives with [`transport_params`].
//! - Slice 39 — the client TLS 1.3 handshake flow ([`tls_handshake`],
//!   RFC 8446 §4, RFC 9001 §4): the pure state machine that sequences the earlier
//!   TLS primitives ([`tls_message`], [`key_agreement`], [`tls_schedule`],
//!   [`tls_finished`], reassembled by [`crypto_stream`]) into the ordered client
//!   handshake. [`tls_handshake::ClientHandshake`] is seeded with the client's
//!   ephemeral X25519 private key and its ClientHello, then fed the server's
//!   flight one message at a time: it enforces the RFC 8446 §4 ordering, runs the
//!   (EC)DHE at ServerHello to report the Handshake-level packet keys, verifies
//!   the server Finished MAC over `ClientHello…CertificateVerify`, and on success
//!   derives the 1-RTT keys and the master / exporter / resumption secrets and
//!   emits the client Finished to send. Fixed to `TLS_AES_128_GCM_SHA256` and
//!   X25519; certificate-chain validation is the caller's job (the raw
//!   Certificate / CertificateVerify are handed back for [`tls_cert_verify`]).
//!   Pure state machine, no IO; HelloRetryRequest, PSK/resumption, client auth and
//!   post-handshake messages are out of scope.
//! - Slice 40 — the HTTP/3 request/response message translation ([`h3_request`],
//!   RFC 9114 §4.1–§4.3): the semantic bridge between an HTTP message and the
//!   QPACK field section a `HEADERS` frame carries — the request-path
//!   counterpart of [`qpack`] (a field *list* → bytes) and [`frame`] (an
//!   *opaque* field block → a frame), mirroring the HTTP/2 `h2::conn`
//!   pseudo-header + fetch handling. [`h3_request::build_request_fields`] /
//!   [`h3_request::encode_request`] build the ordered request field list — the
//!   four request pseudo-headers (`:method`, `:scheme`, `:authority`, `:path`,
//!   §4.3.1) in the impersonated browser's fingerprint order followed by the
//!   regular fields — and QPACK-encode it into a `HEADERS` frame;
//!   [`h3_request::decode_response`] / [`h3_request::validate_response_fields`]
//!   decode a response field block back into the `:status` code and header list,
//!   enforcing the §4.1.2/§4.2/§4.3.2 well-formedness rules (exactly one
//!   `:status` first, no request/unknown pseudo-headers, lower-case names, no
//!   connection-specific fields). Pure functions over the static-only QPACK
//!   codec; request bodies, trailers, the dynamic table, and the transport are
//!   out of scope.
//! - Slice 41 — the QPACK encoder driver ([`qpack_encoder`], RFC 9204 §2.1): the
//!   request-path policy layer over the QPACK codecs — [`qpack_encoder::QpackEncoder`]
//!   owns the encoder's mirror of the decoder's dynamic table, inserts beneficial
//!   header fields (emitting the encoder-stream instructions of [`qpack_stream`],
//!   the "instruction stream wiring" the plan called for), and encodes the field
//!   section against the resulting table via
//!   [`qpack::encode_field_section_dynamic_bounded`] — never referencing an entry
//!   the decoder has not acknowledged unless `SETTINGS_QPACK_BLOCKED_STREAMS`
//!   allows the blocked stream (§2.1.2). It tracks each outstanding section so a
//!   referenced entry is not evicted before acknowledgment (§2.1.3) and advances
//!   the Known Received Count on Section Acknowledgment / Insert Count Increment,
//!   dropping references on Stream Cancellation (§2.1.4, §4.4). Pure state machine
//!   over the two codecs; no IO.
//! - Slice 42 — QUIC outgoing datagram assembly ([`datagram_build`], RFC 9000
//!   §12.2, §14.1): the send-side mirror of [`datagram`].
//!   [`datagram_build::DatagramBuilder`] coalesces the encrypted packet byte
//!   strings from [`packet_crypt::encrypt_packet`] into one outgoing UDP
//!   datagram, enforcing the same §12.2 coalescing rule on the send path (only a
//!   length-delimited long-header packet may be followed by another; a
//!   short-header packet seals the datagram) and bounding the result by the
//!   confirmed path MTU ([`path_mtu`]). It refuses an overflowing packet without
//!   mutation (so the caller flushes and retries in a fresh datagram) and reports
//!   [`datagram_build::DatagramBuilder::initial_padding_shortfall`] so the caller
//!   pads a client Initial to [`datagram::MIN_INITIAL_DATAGRAM_LEN`] (§14.1) with
//!   PADDING frames inside the payload before encryption. This is the "assembling
//!   probe datagrams" step of the transport plan — a PTO probe is a datagram built
//!   here. Pure, no IO.
//! - Slice 43 — the QUIC connection timer scheduler ([`timer`], RFC 9000 §8.2.4,
//!   §10.1, §13.2.1; RFC 9002 §6.2): the pure multiplexer that unifies every
//!   earlier slice's individual deadline — the loss-detection / PTO timer
//!   ([`pto`]), the per-space delayed-ACK timers ([`ack`]), the idle timeout and
//!   the closing/draining period ([`lifecycle`]), and the path-validation timeout
//!   ([`path_validation`]) — into the single OS timer a QUIC event loop arms.
//!   [`timer::ConnectionTimers`] takes each machine's current deadline and answers
//!   [`timer::ConnectionTimers::next`] (the earliest deadline and which
//!   [`timer::TimerKind`] owns it, i.e. exactly when and why to wake next) and
//!   [`timer::ConnectionTimers::fired`] (after waking at `now`, every elapsed timer
//!   ordered earliest-first, so the caller drives each owning state machine). It
//!   holds no cross-timer policy — the connection cancels an irrelevant deadline by
//!   setting it to `None`. Pure, no clock of its own, no IO.
//! - Slice 44 — the QUIC 1-RTT key update state machine ([`crypto_state`],
//!   RFC 9001 §6): the pure state that owns the rotation of the 1-RTT keys over
//!   the life of the connection. Unlike the Initial / Handshake levels, whose keys
//!   live for the handshake only, the 1-RTT keys are periodically advanced to the
//!   next generation ([`key_schedule::next_generation_secret`]) to bound the
//!   packets protected with any one key (§6.6), with a single Key Phase bit
//!   (RFC 9000 §17.3.1) signalling the current generation.
//!   [`crypto_state::OneRttKeyState`] holds our send keys and the peer's receive
//!   keys at the current generation, pre-derives the next receive generation so a
//!   phase-flipped packet can be trial-decrypted ([`crypto_state::RecvKeyDecision`]),
//!   and retains the previous receive generation for a window so a packet reordered
//!   across an update still decrypts (§6.3). It enforces the §6.1 initiation rules
//!   (handshake confirmed; never a second update until the first is acknowledged)
//!   and the §6.2 responder logic (detect a peer-initiated update from a differing
//!   Key Phase bit and advance our own send keys in response, unless we initiated
//!   it). The header-protection key is not rotated by a key update (§6.1). Pure
//!   state machine driven by decoded events; no clock, no IO — the retirement
//!   deadline lives with [`timer`], key discarding for the handshake levels
//!   (RFC 9001 §4.9) with the connection driver.
//! - Slice 45 — the HTTP/3 connection SETTINGS layer ([`settings`], RFC 9114
//!   §7.2.4, RFC 9204 §5, RFC 9220 §3): the typed bridge between the raw
//!   `(identifier, value)` pairs of the [`frame`] SETTINGS codec and the policy
//!   layers that consume them — [`qpack_encoder`] reads the peer's advertised
//!   QPACK dynamic-table capacity and blocked-stream budget, [`h3_request`]
//!   honours the peer's maximum field-section size. [`settings::H3Settings::
//!   for_profile`] builds the local SETTINGS Lumen sends on its control stream,
//!   ordered to match the impersonated browser's fingerprint (mirroring
//!   [`crate::http::h2_settings`]); [`settings::H3Settings::from_pairs`] parses
//!   the peer's SETTINGS into typed values, applying the RFC default for each
//!   absent identifier, re-enforcing the reserved-HTTP/2-identifier and
//!   duplicate-identifier rules (§7.2.4.1) so the typed layer is self-contained,
//!   validating the `SETTINGS_ENABLE_CONNECT_PROTOCOL` value range (RFC 9220
//!   §3), and ignoring unknown / greased identifiers (§7.2.4.2). Pure, no IO —
//!   the control-stream framing and the SETTINGS-before-request sequencing live
//!   with [`h3_stream`].
//! - Slice 46 — the QUIC datagram transport ([`udp`], RFC 9000 §5, §12.2): the
//!   first slice that touches an OS socket. [`udp::DatagramTransport`] is the
//!   message-oriented send/receive seam the connection layer is generic over —
//!   the QUIC analogue of the `Read + Write` byte stream [`crate::h2::conn::
//!   H2Conn`] abstracts over — with a real [`udp::UdpDatagram`] (a connected
//!   [`std::net::UdpSocket`]) in production and a [`udp::MockDatagramTransport`]
//!   (scripted inbound datagrams + captured outbound) in tests. A read timeout
//!   lets one blocking `recv` double as the event loop's timer wait:
//!   [`udp::recv_timeout`] converts [`timer::ConnectionTimers`]' next [`std::
//!   time::Instant`] deadline into the block duration, and [`udp::recv_timed_out`]
//!   recognises the portable "deadline elapsed, fire timers instead" signal.
//!   Reassembly, retransmission, and ordering stay with the connection layer;
//!   the transport only moves opaque datagrams.
//! - Slice 47 — the QUIC event-loop wait ([`event_loop`], RFC 9000 §10.1,
//!   §13.2.1; RFC 9002 §6.2): the glue that ties the [`timer::ConnectionTimers`]
//!   scheduler to the [`udp::DatagramTransport`] read timeout — the single place
//!   that turns "the next QUIC deadline" into "block the socket for exactly this
//!   long, then say why I woke". [`event_loop::DatagramEventLoop`] owns the
//!   transport plus a reused maximum-size receive buffer; each
//!   [`event_loop::DatagramEventLoop::wait`] arms the read timeout from
//!   [`event_loop::next_read_timeout`] ([`timer::ConnectionTimers::next`] +
//!   [`udp::recv_timeout`]) and receives, reporting a [`event_loop::Wakeup`] —
//!   [`event_loop::Wakeup::Datagram`] with the bytes read, or
//!   [`event_loop::Wakeup::TimerExpired`] on the portable read-timeout signal
//!   ([`udp::recv_timed_out`]) so the caller drives whatever
//!   [`timer::ConnectionTimers::fired`] reports. The block duration comes from a
//!   caller-supplied `now` and the final `fired` call stays with the caller, so
//!   the module is clock-free and driven deterministically by a
//!   [`udp::MockDatagramTransport`] in tests. This is the wait *iteration* of the
//!   loop, not the full loop.
//! - Slice 48 — the QUIC connection-level receive dispatch ([`connection`],
//!   RFC 9000 §12.4, §13, §19): the first composition slice of the connection
//!   engine — the single place that owns the connection-wide state machines and
//!   routes each decrypted frame to the machine that owns it.
//!   [`connection::QuicConnection`] holds the per-space [`ack`] generators and
//!   [`crypto_stream`] reassembly buffers, the send-side [`conn_flow`] limits, the
//!   [`conn_id`] sets, the [`path_validation`] validator and anti-amplification
//!   limit, and the [`lifecycle`], and drives them from one decrypted packet at a
//!   time. [`connection::QuicConnection::process_packet`] records the packet number
//!   for the space's acknowledgement, then dispatches each frame: MAX_DATA /
//!   MAX_STREAMS raise our send limits, NEW_CONNECTION_ID / RETIRE_CONNECTION_ID
//!   drive the ID sets (reporting the sequence numbers to retire), PATH_CHALLENGE is
//!   echoed and PATH_RESPONSE validates the path (lifting the anti-amplification
//!   limit), CRYPTO is reassembled per space, and CONNECTION_CLOSE / HANDSHAKE_DONE
//!   move the lifecycle — while ACK (loss detection), the per-stream frames (the
//!   stream manager), and NEW_TOKEN are surfaced in
//!   [`connection::PacketEffects::deferred`] for a later slice.
//!   [`connection::QuicConnection::refresh_timers`] folds every owned machine's
//!   deadline into [`timer::ConnectionTimers`] for [`event_loop`]. Pure and
//!   clock-driven by a caller-supplied `now`; packet decryption and the send path
//!   remain the caller's job.
//! - Slice 49 — the QUIC stream manager ([`stream_manager`], RFC 9000 §2, §3,
//!   §4, §19): the receive-side dispatcher for the per-stream frames slice 48
//!   ([`connection`]) deferred. [`stream_manager::StreamManager`] owns every live
//!   stream's [`stream::RecvStream`] / [`stream::SendStream`] halves, the
//!   connection-wide receive flow-control budget ([`conn_flow::RecvConnFlow`]),
//!   and the two receive stream-count limits ([`conn_flow::RecvStreamLimit`]), and
//!   routes STREAM / RESET_STREAM to the receiving half (lazily creating it with
//!   the receive window advertised for the stream type, enforcing the receive
//!   stream-count limit for a peer-initiated stream and the connection-wide
//!   receive budget across all streams), STOP_SENDING to the sending half
//!   (resetting it and surfacing the RESET_STREAM to send, RFC 9000 §3.5),
//!   MAX_STREAM_DATA to the sending half's per-stream limit, and STREAM_DATA_BLOCKED
//!   as an accepted no-op. A stream's directionality (RFC 9000 §2.1) gates which
//!   half a frame may touch (`STREAM_STATE_ERROR` otherwise). Pure state machine
//!   over the decoded frames; no IO.
//! - Slice 50 — the QUIC send-side frame scheduler ([`send`], RFC 9000 §12.4,
//!   §13.2.1; RFC 9002 §2): the first composition slice of the send path, the
//!   mirror of the connection-level receive dispatch ([`connection`]). Where
//!   [`connection::QuicConnection::process_packet`] routes each *received* frame to
//!   its owning machine, [`send::SendScheduler`] queues the frames the connection
//!   *owes* to send — the acknowledgement an [`ack::AckGenerator`] produced, the
//!   PATH_RESPONSE / RETIRE_CONNECTION_ID a [`connection::PacketEffects`] surfaced,
//!   the CRYPTO / STREAM data a send stream buffered — and packs them, by
//!   descending [`send::SendPriority`] (Close → Ack → Crypto → Control → Stream →
//!   Probe) under a byte budget, into the successive [`packet_payload::PayloadBuilder`]
//!   payloads a packet carries. It validates each frame against the packet type's
//!   permission table (RFC 9000 §12.4) at enqueue, packs FIFO within a priority
//!   (skipping a frame too large for the remaining budget so a smaller later one of
//!   the same class still fits), and reports [`send::SendError::FrameTooLarge`] when
//!   a queued frame cannot fit even an empty packet. Pure state; packet-number
//!   assignment, encryption ([`packet_crypt`]), and datagram coalescing
//!   ([`datagram_build`]) remain later slices.
//! - Slice 51 — the QUIC send engine ([`send_engine`], RFC 9000 §12.2, §14.1,
//!   §17.1; RFC 9002 §2): the pure composition slice that turns the frames a
//!   [`send::SendScheduler`] queued into on-wire, encrypted, coalesced UDP
//!   datagrams and the [`loss::SentPacket`] records that feed loss recovery — the
//!   send-path counterpart of the receive-path [`connection`]. A
//!   [`send_engine::SpaceSender`] owns one packet-number space's monotonic
//!   packet-number counter (RFC 9000 §12.3) and, given a scheduler, a
//!   [`key_schedule::PacketProtectionKeys`] set, and a
//!   [`packet_crypt::ProtectedHeader`], produces one encrypted packet at a time
//!   ([`send_engine::SpaceSender::build_packet`], each carrying its
//!   [`loss::SentPacket`]), drains a whole space into a
//!   [`datagram_build::DatagramBuilder`] coalescing as many packets as the budget
//!   allows ([`send_engine::SpaceSender::fill_datagram`] — long-header packets
//!   coalesce, a short-header 1-RTT packet seals the datagram, RFC 9000 §12.2), or
//!   builds the client's first-flight Initial padded to the
//!   [`datagram::MIN_INITIAL_DATAGRAM_LEN`] floor
//!   ([`send_engine::SpaceSender::build_padded_initial`], RFC 9000 §14.1). Each
//!   payload is budgeted against [`send_engine::max_packet_overhead`] so coalescing
//!   never overflows. Pure state, clock-driven by a caller-supplied `now`; the
//!   socket write over [`event_loop`] and the `h3_do_request` dispatch are the next
//!   slice.
//! - Slice 52 — the QUIC send-path datagram flush ([`send_path`], RFC 9000 §12.2,
//!   §12.4, §14.1; RFC 9002 §2, §6): the send-path counterpart of the receive-path
//!   [`connection`]. Where [`connection::QuicConnection::process_packet`] decrypts
//!   one inbound datagram and routes its frames inward, [`send_path::flush`] drives
//!   the per-space [`send_engine`] outward — folding each space's pending frames
//!   through [`send_engine::SpaceSender::fill_datagram`] into one
//!   [`datagram_build::DatagramBuilder`] (long-header Initial / Handshake packets
//!   coalesce, the first short-header 1-RTT packet seals the datagram, RFC 9000
//!   §12.2), writing the coalesced bytes over the
//!   [`udp::DatagramTransport`], and recording each [`loss::SentPacket`] into its
//!   space's [`loss::SentPacketRegistry`] and [`recovery::CongestionController`]
//!   `bytes_in_flight`. [`send_path::send_padded_initial`] is the client's
//!   first-flight path, padding the lone Initial to the
//!   [`datagram::MIN_INITIAL_DATAGRAM_LEN`] floor (RFC 9000 §14.1). Pure apart from
//!   the mockable transport write; the flush is clock-driven by a caller-supplied
//!   `now`.
//! - Slice 53 — the QUIC receive-path datagram ingest ([`recv_path`], RFC 9000
//!   §8.1, §10.1, §12.2, §12.4; RFC 9001 §5.5): the receive-path counterpart of the
//!   send-path [`send_path::flush`]. Where [`send_path::flush`] folds each space's
//!   pending frames outward into coalesced datagrams, [`recv_path::ingest_datagram`]
//!   draws one inbound datagram inward — crediting its bytes to the connection's
//!   anti-amplification limit and idle timer
//!   ([`connection::QuicConnection::on_datagram_received`]), then walking the
//!   coalesced packets front to back (RFC 9000 §12.2): it peeks each header to pick
//!   the [`recv_path::SpaceKeys`] from a [`recv_path::RecvKeyRing`], decrypts through
//!   [`packet_crypt::decrypt_packet`], parses the authenticated payload into frames,
//!   checks each against the packet type's permission table (RFC 9000 §12.4), and
//!   dispatches them through [`connection::QuicConnection::process_packet`], merging
//!   every packet's [`connection::PacketEffects`] into one [`recv_path::IngestReport`].
//!   Only an authenticated packet's content raises a [`recv_path::IngestError`] (a
//!   malformed frame, a barred frame, or a connection-level violation); a packet
//!   with no keys yet is counted undecryptable, and an AEAD failure or a malformed
//!   coalesced header is silently discarded (RFC 9001 §5.5.2). Pure apart from the
//!   caller-supplied `now`.
//! - Slice 54 — the QUIC connection driver ([`driver`], RFC 9000 §8.2.4, §10.1,
//!   §10.2, §13.2.1; RFC 9002 §6.2): the loop body that decides *when* to ingest a
//!   datagram and *when* to act on a timer, tying the receive path [`recv_path`] to
//!   the timer scheduler [`timer`] over the event-loop wait [`event_loop`].
//!   [`driver::ConnectionDriver`] owns the event loop, the [`connection`] receiver
//!   state, the [`pto::LossDetection`] (which owns the send-side registries and PTO
//!   timer), the unified [`timer::ConnectionTimers`], and the [`recv_path::RecvKeyRing`].
//!   [`driver::ConnectionDriver::wait`] refreshes both timer sources — the
//!   connection's own deadlines ([`connection::QuicConnection::refresh_timers`]) and
//!   the loss-detection / PTO timer the connection leaves untouched
//!   ([`pto::LossDetection::set_loss_detection_timer`]) — arms the socket read
//!   timeout for the earliest, and blocks; on a datagram wake
//!   [`driver::ConnectionDriver::ingest`] routes it through the receive path, and on
//!   a timer wake [`driver::ConnectionDriver::dispatch_timers`] drives each elapsed
//!   timer into its owning machine (PTO probe / declared loss, owed ACK, path
//!   abandon, idle close, draining discard) and reports the send-side obligations as
//!   [`driver::DriverAction`]s. Clock-free apart from the caller-supplied `now`;
//!   deterministic under a [`udp::MockDatagramTransport`]. The send-path flush
//!   ([`send_path::flush`], which borrows the per-space send state) and the
//!   `h3_do_request` dispatch remain the caller's job.
//! - Slice 55 — the QUIC send-side connection state ([`send_state`], RFC 9000
//!   §12.3, §14.1; RFC 9002 §7): the owner of everything the send path needs that
//!   the [`driver`] does not. [`send_state::ConnectionSendState`] holds one
//!   send-state per installed packet-number space (the [`send_engine::SpaceSender`]
//!   packet-number counter, the [`send::SendScheduler`] frame queue, the
//!   send-direction [`key_schedule::PacketProtectionKeys`], and the space's
//!   [`recovery::CongestionController`]) plus the header fields every packet shares
//!   (version, Destination / Source Connection IDs, Initial token). A space is
//!   absent until [`send_state::ConnectionSendState::install`] derives its keys and
//!   falls away again on [`send_state::ConnectionSendState::discard`] (RFC 9001
//!   §4.9). [`send_state::ConnectionSendState::flush`] borrows the per-space
//!   registries from the driver's [`pto::LossDetection`]
//!   ([`pto::LossDetection::registries_mut`]) and folds the installed spaces through
//!   [`send_path::flush`] in send order — Initial, Handshake, Application Data — so
//!   the long-header packets coalesce and the 1-RTT space seals the datagram (RFC
//!   9000 §12.2); [`send_state::ConnectionSendState::send_padded_initial`] is the
//!   client's first-flight path. Pure apart from the mockable transport write.
//! - Slice 56 — the client connect bootstrap ([`client_bootstrap`], RFC 9000
//!   §7.2/§7.3, RFC 9001 §5.2, RFC 9114 §3.2): the one place that turns a bare
//!   `(transport, server name, trust store)` triple into a ready-to-drive
//!   [`conn_connect::ConnectDriver`]. It invents the client's random Initial
//!   Destination/Source Connection IDs, an ephemeral X25519 key pair, and a
//!   real TLS 1.3 ClientHello (SNI, `h3` ALPN, a single X25519 key_share, the
//!   mandatory extensions, and the `quic_transport_parameters` whose
//!   `initial_source_connection_id` echoes the client SCID), derives the Initial
//!   keys from the DCID (sending with the client secret, receiving with the
//!   server secret), and wires the receive/loss/TLS stack into the driver. The
//!   caller only has to [`connect`](conn_connect::ConnectDriver::connect).
//! - Slice 57 — the real-transport orchestrator ([`client_transport`], RFC 9114
//!   §3.3/§4.1): [`client_transport::h3_do_request`] resolves the authority
//!   through the injected [`lumen_core::ext::DnsResolver`], opens a real
//!   [`udp::UdpDatagram`] socket to the first address, loads the bundled Mozilla
//!   roots ([`mozilla_roots::mozilla_trust_anchors`]), and drives
//!   [`client_bootstrap::connect_client`] → [`client_request::connect_and_fetch`]
//!   to a single [`h3_exchange::H3Response`]. Its transport-generic core
//!   `h3_exchange` runs the whole composition over a
//!   [`udp::MockDatagramTransport`] in tests.
//! - Slice 58 — the [`h3_exchange::H3Response`] → crate `Response` mapping at the
//!   dispatch boundary in `lib.rs`: the one place that bridges the protocol-native
//!   response this module returns onto the crate-private `Response` shape the H1/H2
//!   branches produce (`impl From<H3Response> for Response`), decoding the opaque
//!   header octets (RFC 9114 §4.2) into text with a UTF-8 lossy conversion and
//!   dropping the interim (`1xx`) responses and trailer section the crate `Response`
//!   has no slot for — the same surface the H2 path exposes.
//! - Slice 59a — the Alt-Svc dispatch decision in `lib.rs` (`try_h3_dispatch`):
//!   the QUIC leg the H1/H2 branches consult before their own connect. It routes
//!   an origin onto [`client_transport::h3_do_request`] only when a fresh cached
//!   `h3` alternative exists ([`alt_svc::AltSvcCache`], keyed by
//!   [`alt_svc::origin_key`] and resolved to a concrete target through
//!   [`alt_svc::AltSvcEntry::connect_target`]), maps the [`h3_exchange::H3Response`]
//!   onto the crate `Response`, and on a failed leg evicts the "broken"
//!   alternative (RFC 7838 §2.4) and returns `None` so the caller falls back to
//!   H2/H1.1. Not yet called from `fetch_single` — the same deferred-caller step
//!   Slice 58 took before it.
//! - Slice 59b+ (planned) — wiring `try_h3_dispatch` into `fetch_single`: giving
//!   the `HttpClient` the cache, scanning H2/H1.1 responses for `Alt-Svc` to
//!   populate it, and converting the request's header block into the `(name,
//!   value)` byte pairs the dispatch takes.
//!
//! The codecs here are the shared foundation: QUIC varints delimit both
//! transport-layer fields and HTTP/3 frames, the QUIC frame codec carries the
//! transport payload, the HTTP/3 frame codec carries an opaque QPACK field
//! block, [`qpack`] turns that block into header fields, and [`alt_svc`]
//! decides when an origin is eligible for the QUIC path at all.

pub mod ack;
pub mod alt_svc;
pub mod client_bootstrap;
pub mod client_request;
pub mod client_transport;
pub mod conn_cert_auth;
pub mod conn_connect;
pub mod conn_flow;
pub mod conn_handshake;
pub mod conn_id;
pub mod conn_tls;
pub mod conn_turn;
pub mod connection;
pub mod crypto_state;
pub mod crypto_stream;
pub mod datagram;
pub mod datagram_build;
pub mod driver;
pub mod event_loop;
pub mod frame;
pub mod h3_exchange;
pub mod h3_request;
pub mod h3_stream;
pub mod key_agreement;
pub mod key_schedule;
pub mod lifecycle;
pub mod loss;
pub mod mozilla_roots;
pub mod packet;
pub mod packet_crypt;
pub mod packet_number;
pub mod packet_payload;
pub mod packet_protect;
pub mod path_migration;
pub mod path_mtu;
pub mod path_validation;
pub mod pto;
pub mod qpack;
pub mod qpack_encoder;
pub mod qpack_stream;
pub mod quic_frame;
pub mod recovery;
pub mod recv_path;
pub mod request_dispatch;
pub mod request_driver;
pub mod request_exchange;
pub mod request_mux;
pub mod request_pump;
pub mod request_turn;
pub mod retry;
pub mod send;
pub mod send_engine;
pub mod send_path;
pub mod send_state;
pub mod settings;
pub mod stream;
pub mod stream_manager;
pub mod tls_cert_verify;
pub mod tls_finished;
pub mod tls_handshake;
pub mod tls_message;
pub mod timer;
pub mod tls_schedule;
pub mod transport_params;
pub mod udp;
pub mod varint;
pub mod version_nego;
pub mod x509_basic_constraints;
pub mod x509_chain;
pub mod x509_critical_ext;
pub mod x509_ext_key_usage;
pub mod x509_hostname;
pub mod x509_key_usage;
pub mod x509_name_chain;
pub mod x509_spki;
pub mod x509_trust_anchor;
pub mod x509_validity;
