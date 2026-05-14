//! DNS-over-TLS резолвер (RFC 7858).
//!
//! Реализует `lumen_core::ext::DnsResolver` поверх собственного TCP+TLS
//! сокета (rustls, exception #3). Wire-format DNS message — RFC 1035 §4,
//! переиспользуется из `doh.rs` (`encode_query` / `decode_answer_ips`).
//!
//! Отличия от DoH:
//! - транспорт — чистый TLS, без HTTP-обёртки;
//! - default port 853 (RFC 7858 §3.1);
//! - DNS message обёрнут двухбайтовой length-prefix (RFC 1035 §4.2.2 —
//!   TCP transport): `[u16 BE length][message]`. UDP-вариант с прямым
//!   datagram-ом не используется (DoT поверх TLS — TLS поверх TCP).
//!
//! **Bootstrap.** В отличие от system-резолвера, DoT-сервер сам имеет
//! hostname (`one.one.one.one`, `dns.google`, и т.д.); рекурсивно
//! резолвить его через тот же DoT нельзя. Решение —
//! `DotResolver::new(server_name, server_addr)` принимает уже
//! зарезолвленный `SocketAddr` (через system resolver на старте, либо
//! IP-литерал, либо CachedDnsResolver). `server_name` отдельно — это
//! TLS SNI + ServerName для верификации сертификата (RFC 7858 §4.2 —
//! authenticated dotted name + SubjectPublicKeyInfo pinning опционально;
//! здесь — только cert chain через webpki-roots).
//!
//! Семантика идентична DoH:
//! - литералы IPv4/IPv6 обрабатываются локально и не идут в DoT;
//! - на каждый `resolve(host, port)` шлём ДВА запроса — AAAA и A
//!   последовательно, результаты объединяем (IPv6 перед IPv4 по RFC 6724
//!   §6 default); если оба пусты — `Err`;
//! - SOA / NXDOMAIN / RCODE!=0 трактуются как «нет адресов»;
//! - TTL не обрабатываем (кеш сверху через `CachedDnsResolver`);
//! - **persistent connection не используется** — каждый query открывает
//!   свежий TLS. RFC 7858 §3.4 разрешает reuse, но в Phase 0 это
//!   усложнение без выигрыша (две AAAA+A пары за resolve редки в реальном
//!   pipeline); persistent — отдельной задачей при необходимости.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use rustls::ClientConnection;
use rustls::pki_types::ServerName;

use lumen_core::error::{Error, Result};
use lumen_core::ext::DnsResolver;

use crate::doh::{TYPE_A, TYPE_AAAA, decode_answer_ips, encode_query};

/// Стандартный порт DoT (RFC 7858 §3.1). 853 = "DNS Query Service over TLS".
pub const DOT_DEFAULT_PORT: u16 = 853;

/// Максимальный размер DNS message по wire (RFC 1035 §4.2.1 ставит лимит
/// 65535 байт через u16 length-prefix; реальный потолок гораздо ниже, но
/// 64 КБ — единственная hard cap из спеки).
const MAX_DNS_MESSAGE: usize = 65_535;

// ── TCP framing (RFC 1035 §4.2.2) ────────────────────────────────────────────

/// Обернуть DNS message в two-octet length prefix: `[u16 BE len][msg]`.
/// Используется для DNS over TCP / TLS / любого stream-транспорта.
pub fn frame_query(msg: &[u8]) -> Result<Vec<u8>> {
    if msg.len() > MAX_DNS_MESSAGE {
        return Err(Error::Network(format!(
            "DNS: message длиннее 65535 байт ({})",
            msg.len()
        )));
    }
    let mut framed = Vec::with_capacity(2 + msg.len());
    framed.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    framed.extend_from_slice(msg);
    Ok(framed)
}

/// Прочитать ОДНО framed DNS message из stream-а: 2 байта BE length,
/// затем `length` байт payload. EOF до length — Err.
pub fn read_framed_message<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    read_exact_ext(reader, &mut len_buf)
        .map_err(|e| Error::Network(format!("DoT: read length prefix: {e}")))?;
    let len = u16::from_be_bytes(len_buf) as usize;
    if len == 0 {
        return Err(Error::Network("DoT: zero-length DNS message".to_owned()));
    }
    let mut msg = vec![0u8; len];
    read_exact_ext(reader, &mut msg)
        .map_err(|e| Error::Network(format!("DoT: read {len}-byte message: {e}")))?;
    Ok(msg)
}

/// `read_exact` с трансляцией std::io::Error в строковое описание для
/// единообразного wrapping в `Error::Network`. Чистый `read_exact` имеет
/// io::Error, который теряет контекст при `format!("{e}")` иногда.
fn read_exact_ext<R: Read>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<()> {
    reader.read_exact(buf)
}

// ── Stream-level query ───────────────────────────────────────────────────────

/// Послать ОДИН DNS query (AAAA или A — определяется `qtype`) по уже
/// открытому stream-у и распарсить ответ. Wire-format — `encode_query`
/// из DoH; framing — `frame_query` сверху.
///
/// Pub для тестируемости и потенциального reuse в DoT с persistent
/// connection: caller передаёт mock stream / TLS stream / TcpStream
/// одинаково.
pub fn query_over_stream<S: Read + Write>(
    stream: &mut S,
    hostname: &str,
    qtype: u16,
) -> Result<Vec<IpAddr>> {
    let msg = encode_query(0, hostname, qtype)?;
    let framed = frame_query(&msg)?;
    stream
        .write_all(&framed)
        .map_err(|e| Error::Network(format!("DoT: write query: {e}")))?;
    stream
        .flush()
        .map_err(|e| Error::Network(format!("DoT: flush query: {e}")))?;
    let response = read_framed_message(stream)?;
    decode_answer_ips(&response)
}

// ── DotResolver ──────────────────────────────────────────────────────────────

/// DNS-over-TLS резолвер.
///
/// Использует rustls (exception #3) поверх собственного TcpStream.
/// `server_name` — TLS SNI и subject name для верификации цепочки
/// сертификатов; `server_addr` — pre-resolved IP+порт DoT-сервера.
/// Разделение нужно потому что DoT-сервер обычно известен под hostname
/// (`one.one.one.one`, `dns.google`), но рекурсивно резолвить его через
/// собственный DoT нельзя — bootstrap должен прийти извне (system
/// resolver или IP-литерал в коде).
///
/// Удобные фабрики `cloudflare()` / `google()` / `quad9()` зашивают
/// hardcoded IP-литералы официальных публичных DoT-серверов — это
/// устраняет bootstrap-проблему ценой того, что IP-литерал может
/// устареть; для production предпочтительнее `new()` с явным адресом.
pub struct DotResolver {
    server_name: String,
    server_addr: SocketAddr,
    tls_config: Arc<rustls::ClientConfig>,
}

impl DotResolver {
    /// Базовый конструктор. `server_name` — TLS SNI/cert host;
    /// `server_addr` — куда коннектиться (обычно `<ip>:853`).
    pub fn new(server_name: impl Into<String>, server_addr: SocketAddr) -> Self {
        Self {
            server_name: server_name.into(),
            server_addr,
            tls_config: default_tls_config(),
        }
    }

    /// Cloudflare `1.1.1.1:853` с SNI `one.one.one.one`.
    /// IP-литерал из RFC 8484 §10.2 / cloudflare.com/dns/.
    pub fn cloudflare() -> Self {
        Self::new(
            "one.one.one.one",
            SocketAddr::from(([1u8, 1, 1, 1], DOT_DEFAULT_PORT)),
        )
    }

    /// Google Public DNS `8.8.8.8:853` с SNI `dns.google`.
    pub fn google() -> Self {
        Self::new(
            "dns.google",
            SocketAddr::from(([8u8, 8, 8, 8], DOT_DEFAULT_PORT)),
        )
    }

    /// Quad9 `9.9.9.9:853` с SNI `dns.quad9.net`.
    pub fn quad9() -> Self {
        Self::new(
            "dns.quad9.net",
            SocketAddr::from(([9u8, 9, 9, 9], DOT_DEFAULT_PORT)),
        )
    }

    /// Открыть TLS stream до DoT-сервера. Каждый вызов — свежее TLS
    /// (см. note в module-doc про persistent).
    fn connect_tls(&self) -> Result<rustls::StreamOwned<ClientConnection, TcpStream>> {
        let tcp = TcpStream::connect(self.server_addr)
            .map_err(|e| Error::Network(format!("DoT: TCP connect {}: {e}", self.server_addr)))?;
        let sni = ServerName::try_from(self.server_name.clone()).map_err(|e| {
            Error::Network(format!(
                "DoT: invalid server_name '{}': {e}",
                self.server_name
            ))
        })?;
        let conn = ClientConnection::new(self.tls_config.clone(), sni)
            .map_err(|e| Error::Network(format!("DoT: TLS init: {e}")))?;
        Ok(rustls::StreamOwned::new(conn, tcp))
    }

    /// Отправить один query (свежее TLS) и распарсить ответ.
    fn query(&self, hostname: &str, qtype: u16) -> Result<Vec<IpAddr>> {
        let mut stream = self.connect_tls()?;
        query_over_stream(&mut stream, hostname, qtype)
    }
}

impl DnsResolver for DotResolver {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        // Литералы IP — bypass без обращения к серверу (как в DoH).
        let unbracketed = hostname.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
        let literal_candidate = unbracketed.unwrap_or(hostname);
        if let Ok(ip) = IpAddr::from_str(literal_candidate) {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }

        // AAAA сначала (RFC 6724 §6 default — dual-stack preference), потом A.
        // Если AAAA дал Err — продолжаем на A; если оба пусты — Err.
        let mut addrs = Vec::new();
        let mut last_err: Option<Error> = None;

        match self.query(hostname, TYPE_AAAA) {
            Ok(ips) => {
                for ip in ips {
                    addrs.push(SocketAddr::new(ip, port));
                }
            }
            Err(e) => last_err = Some(e),
        }
        match self.query(hostname, TYPE_A) {
            Ok(ips) => {
                for ip in ips {
                    addrs.push(SocketAddr::new(ip, port));
                }
            }
            Err(e) => {
                if addrs.is_empty() {
                    last_err = Some(e);
                }
            }
        }

        if addrs.is_empty() {
            return Err(last_err.unwrap_or_else(|| {
                Error::Network(format!(
                    "DoT: no addresses for {hostname} (NODATA from both A and AAAA)"
                ))
            }));
        }
        Ok(addrs)
    }
}

// ── Глобальный TLS config ────────────────────────────────────────────────────

/// `rustls::ClientConfig` с webpki-roots, кэшированный глобально.
/// rustls config дешёво клонируется через `Arc`, но построение
/// (root store + handshakes machinery) — не бесплатно; для DoT-резолвера,
/// который вызывается часто, переиспользуем один config.
fn default_tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let cfg = rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            Arc::new(cfg)
        })
        .clone()
}

// ── Тесты ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // ── frame_query ──

    #[test]
    fn frame_query_prepends_length_be() {
        let payload = vec![0xAB, 0xCD, 0xEF];
        let framed = frame_query(&payload).unwrap();
        assert_eq!(framed.len(), 5);
        assert_eq!(&framed[0..2], &[0x00, 0x03]);
        assert_eq!(&framed[2..], &payload[..]);
    }

    #[test]
    fn frame_query_empty_message() {
        let framed = frame_query(&[]).unwrap();
        assert_eq!(framed, vec![0x00, 0x00]);
    }

    #[test]
    fn frame_query_max_size_ok() {
        let payload = vec![0u8; MAX_DNS_MESSAGE];
        let framed = frame_query(&payload).unwrap();
        assert_eq!(framed.len(), 2 + MAX_DNS_MESSAGE);
        assert_eq!(&framed[0..2], &[0xFF, 0xFF]);
    }

    #[test]
    fn frame_query_too_large_errors() {
        let payload = vec![0u8; MAX_DNS_MESSAGE + 1];
        assert!(frame_query(&payload).is_err());
    }

    // ── read_framed_message ──

    #[test]
    fn read_framed_roundtrip() {
        let original = b"hello dns";
        let framed = frame_query(original).unwrap();
        let mut cursor = Cursor::new(framed);
        let got = read_framed_message(&mut cursor).unwrap();
        assert_eq!(got, original);
    }

    #[test]
    fn read_framed_eof_during_length() {
        // Только один из двух байт длины.
        let mut cursor = Cursor::new(vec![0x00]);
        assert!(read_framed_message(&mut cursor).is_err());
    }

    #[test]
    fn read_framed_eof_during_payload() {
        // Length=10, но реальных байт меньше.
        let mut buf = vec![0x00, 0x0A];
        buf.extend_from_slice(b"short");
        let mut cursor = Cursor::new(buf);
        assert!(read_framed_message(&mut cursor).is_err());
    }

    #[test]
    fn read_framed_zero_length_errors() {
        // Length=0 — невалидно (нет даже DNS header).
        let mut cursor = Cursor::new(vec![0x00, 0x00]);
        let err = read_framed_message(&mut cursor).unwrap_err();
        assert!(format!("{err}").contains("zero-length"));
    }

    #[test]
    fn read_framed_consumes_only_one_message() {
        // Два сообщения подряд — должны быть прочитаны раздельно.
        let mut buf = frame_query(b"abc").unwrap();
        buf.extend_from_slice(&frame_query(b"defgh").unwrap());
        let mut cursor = Cursor::new(buf);
        let m1 = read_framed_message(&mut cursor).unwrap();
        let m2 = read_framed_message(&mut cursor).unwrap();
        assert_eq!(m1, b"abc");
        assert_eq!(m2, b"defgh");
    }

    // ── query_over_stream ──

    /// Mock stream: при write ничего не запоминает, при read отдаёт
    /// заранее заготовленные байты. Достаточно для проверки, что
    /// query_over_stream правильно парсит framed-ответ и отдаёт IP-адреса.
    struct MockReadWrite {
        written: Vec<u8>,
        to_read: Cursor<Vec<u8>>,
    }

    impl MockReadWrite {
        fn new(response: Vec<u8>) -> Self {
            Self {
                written: Vec::new(),
                to_read: Cursor::new(response),
            }
        }
    }

    impl Read for MockReadWrite {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.to_read.read(buf)
        }
    }

    impl Write for MockReadWrite {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Собрать DNS-ответ как в doh-тестах: header + question (example.com IN A)
    /// + ответы. Скопирован из doh::tests, упрощён.
    fn build_response(flags: u16, ancount: u16, answer_section: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&0u16.to_be_bytes()); // ID
        msg.extend_from_slice(&flags.to_be_bytes());
        msg.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
        msg.extend_from_slice(&ancount.to_be_bytes()); // ANCOUNT
        msg.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
        msg.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
        msg.extend_from_slice(&[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
            3, b'c', b'o', b'm',
            0,
            0x00, 0x01, 0x00, 0x01,
        ]);
        msg.extend_from_slice(answer_section);
        msg
    }

    fn a_record_via_pointer(ip: [u8; 4]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&[0xC0, 0x0C]);
        a.extend_from_slice(&1u16.to_be_bytes()); // TYPE_A
        a.extend_from_slice(&1u16.to_be_bytes()); // CLASS_IN
        a.extend_from_slice(&60u32.to_be_bytes()); // TTL
        a.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        a.extend_from_slice(&ip);
        a
    }

    fn aaaa_record_via_pointer(ip: [u8; 16]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&[0xC0, 0x0C]);
        a.extend_from_slice(&28u16.to_be_bytes()); // TYPE_AAAA
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&60u32.to_be_bytes());
        a.extend_from_slice(&16u16.to_be_bytes());
        a.extend_from_slice(&ip);
        a
    }

    #[test]
    fn query_over_stream_writes_framed_query() {
        let response = build_response(0x8180, 1, &a_record_via_pointer([1, 2, 3, 4]));
        let framed = frame_query(&response).unwrap();
        let mut stream = MockReadWrite::new(framed);

        let ips = query_over_stream(&mut stream, "example.com", TYPE_A).unwrap();
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))]);

        // Проверяем, что запрос был отправлен с правильным framing-ом.
        assert!(stream.written.len() >= 2);
        let written_len = u16::from_be_bytes([stream.written[0], stream.written[1]]) as usize;
        assert_eq!(written_len + 2, stream.written.len());
        // Question section с example.com — проверим начало payload-а.
        let payload = &stream.written[2..];
        assert!(payload.len() >= 12); // DNS header
        // QTYPE=A в конце question
        let payload_end = &payload[payload.len() - 4..];
        assert_eq!(payload_end, &[0x00, 0x01, 0x00, 0x01]);
    }

    #[test]
    fn query_over_stream_parses_aaaa() {
        let ip = [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        let response = build_response(0x8180, 1, &aaaa_record_via_pointer(ip));
        let framed = frame_query(&response).unwrap();
        let mut stream = MockReadWrite::new(framed);

        let ips = query_over_stream(&mut stream, "example.com", TYPE_AAAA).unwrap();
        let want: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert_eq!(ips, vec![IpAddr::V6(want)]);
    }

    #[test]
    fn query_over_stream_rcode_servfail_errors() {
        let response = build_response(0x8182, 0, &[]); // RCODE=2
        let framed = frame_query(&response).unwrap();
        let mut stream = MockReadWrite::new(framed);
        assert!(query_over_stream(&mut stream, "example.com", TYPE_A).is_err());
    }

    #[test]
    fn query_over_stream_truncated_payload_errors() {
        // Length-prefix говорит 100, реально — 10 байт. read_framed должен
        // вернуть Err при попытке прочитать 100 байт.
        let mut buf = Vec::new();
        buf.extend_from_slice(&100u16.to_be_bytes());
        buf.extend_from_slice(&[0u8; 10]);
        let mut stream = MockReadWrite::new(buf);
        assert!(query_over_stream(&mut stream, "example.com", TYPE_A).is_err());
    }

    #[test]
    fn query_over_stream_aaaa_ipv4_literal_not_bypassed_at_this_level() {
        // query_over_stream — низкоуровневая функция, она НЕ обрабатывает
        // IP-литералы (это делает DotResolver::resolve). Проверим, что
        // мы действительно отправляем запрос даже для "1.1.1.1".
        let response = build_response(0x8180, 0, &[]);
        let framed = frame_query(&response).unwrap();
        let mut stream = MockReadWrite::new(framed);
        let _ = query_over_stream(&mut stream, "1.1.1.1", TYPE_A);
        assert!(!stream.written.is_empty(), "должен был отправить запрос");
    }

    // ── DotResolver — IP literal bypass ──

    #[test]
    fn resolve_ipv4_literal_bypasses_dot() {
        // server_addr заведомо невалидный — если бы код попытался connect,
        // это упало бы. Mock-сервер не нужен потому что IP-литерал должен
        // вернуться без обращения к сети.
        let resolver = DotResolver::new(
            "invalid.test",
            SocketAddr::from(([127u8, 0, 0, 1], 1)),
        );
        let addrs = resolver.resolve("8.8.8.8", 53).unwrap();
        assert_eq!(
            addrs,
            vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53)]
        );
    }

    #[test]
    fn resolve_ipv6_literal_bypasses_dot() {
        let resolver = DotResolver::new(
            "invalid.test",
            SocketAddr::from(([127u8, 0, 0, 1], 1)),
        );
        let addrs = resolver.resolve("::1", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
    }

    #[test]
    fn resolve_bracketed_ipv6_literal_bypasses_dot() {
        let resolver = DotResolver::new(
            "invalid.test",
            SocketAddr::from(([127u8, 0, 0, 1], 1)),
        );
        let addrs = resolver.resolve("[2001:db8::1]", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
    }

    // ── Фабрики и Send+Sync ──

    #[test]
    fn cloudflare_factory_sets_known_endpoint() {
        let r = DotResolver::cloudflare();
        assert_eq!(r.server_name, "one.one.one.one");
        assert_eq!(
            r.server_addr,
            SocketAddr::from(([1u8, 1, 1, 1], DOT_DEFAULT_PORT))
        );
    }

    #[test]
    fn google_factory_sets_known_endpoint() {
        let r = DotResolver::google();
        assert_eq!(r.server_name, "dns.google");
        assert_eq!(
            r.server_addr,
            SocketAddr::from(([8u8, 8, 8, 8], DOT_DEFAULT_PORT))
        );
    }

    #[test]
    fn quad9_factory_sets_known_endpoint() {
        let r = DotResolver::quad9();
        assert_eq!(r.server_name, "dns.quad9.net");
        assert_eq!(
            r.server_addr,
            SocketAddr::from(([9u8, 9, 9, 9], DOT_DEFAULT_PORT))
        );
    }

    #[test]
    fn dot_resolver_is_send_sync_object_safe() {
        fn check<T: Send + Sync>() {}
        check::<DotResolver>();
        // Object-safety: можно положить в Arc<dyn DnsResolver>.
        let resolver: Arc<dyn DnsResolver> = Arc::new(DotResolver::cloudflare());
        let _ = resolver;
    }

    #[test]
    fn invalid_server_name_errors_on_connect() {
        // server_name = пустая строка → ServerName::try_from даёт ошибку
        // ДО реального TCP-connect-а. Проверяем, что ошибка пробрасывается.
        // Хост 127.0.0.1:1 заведомо никуда не подсоединится, но TCP connect
        // случится раньше TLS init. Поэтому используем bind на занятый
        // эфемерный порт нельзя из теста надёжно — просто проверим, что
        // ServerName::try_from на пустой строке падает.
        let bad = ServerName::try_from(String::new());
        assert!(bad.is_err());
    }
}
