//! DNS-over-HTTPS резолвер (RFC 8484).
//!
//! Реализует `lumen_core::ext::DnsResolver` поверх любого
//! `NetworkTransport` (типично — собственный `HttpClient`, у которого
//! резолвер — `SystemDnsResolver` или CachedDnsResolver для bootstrap-а
//! IP-адреса DoH endpoint-а; **endpoint и сам не должен резолвиться через
//! `DohResolver`**, иначе бесконечная рекурсия).
//!
//! Wire format запроса/ответа — RFC 1035 §4 (стандартный DNS message
//! поверх HTTP). DoH-обёртка по RFC 8484 §4: GET с `?dns=<base64url>`
//! и Accept/Content-Type `application/dns-message`. POST умышленно не
//! используем — текущий `HttpClient` только GET, и base64url-encoded
//! query короче 2048 байт укладывается в любой URL-лимит.
//!
//! Семантика:
//! - литералы IPv4/IPv6 (`127.0.0.1`, `::1`, `2001:db8::1`) обрабатываются
//!   локально и не идут в DoH (стандартное поведение всех резолверов);
//! - на каждый `resolve(host, port)` шлём ДВА запроса — A (QTYPE=1) и
//!   AAAA (QTYPE=28) — последовательно, результаты объединяем (IPv6
//!   перед IPv4 — dual-stack preference, RFC 6724 §6 default); если оба
//!   пусты — `Err`;
//! - SOA / NXDOMAIN / RCODE!=0 трактуются как «нет адресов» (caller
//!   увидит Err или пустой Vec);
//! - TTL и DNSSEC-валидация в Phase 0 не обрабатываются — кеш сверху
//!   (`CachedDnsResolver`) ставит свой TTL, AD-bit просто игнорируется.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use lumen_core::error::{Error, Result};
use lumen_core::ext::{DnsResolver, NetworkTransport};
use lumen_core::url::Url;

// ── Wire format (RFC 1035 §4) ────────────────────────────────────────────────

/// QTYPE / TYPE значения, нужные нам. RFC 1035 §3.2.2 + RFC 3596 §2.1.
pub(crate) const TYPE_A: u16 = 1;
pub(crate) const TYPE_AAAA: u16 = 28;
pub(crate) const CLASS_IN: u16 = 1;

/// Закодировать стандартный DNS query — header + одна question. RD=1
/// (recursion desired), остальные флаги нули; transaction_id для DoH
/// RFC 8484 §4.1 советует 0 (HTTP сам трекает), но любое значение
/// валидно — сервер вернёт то же ID в ответе.
pub(crate) fn encode_query(transaction_id: u16, qname: &str, qtype: u16) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(64);
    // Header (12 байт)
    out.extend_from_slice(&transaction_id.to_be_bytes());
    let flags: u16 = 0x0100; // QR=0, Opcode=0, AA=0, TC=0, RD=1, RA=0, Z=0, RCODE=0
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    out.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    write_qname(&mut out, qname)?;
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&CLASS_IN.to_be_bytes());
    Ok(out)
}

/// Сериализовать QNAME как последовательность length-prefixed labels,
/// завершённую нулевым length byte (root label). RFC 1035 §3.1.
fn write_qname(out: &mut Vec<u8>, qname: &str) -> Result<()> {
    let trimmed = qname.trim_end_matches('.');
    if trimmed.is_empty() {
        // root domain "" / "." → одна записи нулевой длины.
        out.push(0);
        return Ok(());
    }
    for label in trimmed.split('.') {
        if label.is_empty() {
            return Err(Error::Network(format!(
                "DNS: пустой label в '{qname}' (consecutive dots)"
            )));
        }
        let bytes = label.as_bytes();
        if bytes.len() > 63 {
            return Err(Error::Network(format!(
                "DNS: label '{label}' длиннее 63 байт"
            )));
        }
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
    out.push(0);
    if out.len() > 255 + 12 + 4 {
        // Грубая верхняя оценка: 255 байт на name + header(12) + qtype/qclass(4).
        return Err(Error::Network(format!(
            "DNS: QNAME '{qname}' длиннее 255 байт"
        )));
    }
    Ok(())
}

/// Распакованный DNS-ответ — без CNAME-цепочек, только IP-адреса из
/// answer section. Пустой вектор = legitimate «нет адресов» (NODATA);
/// `Err` = wire-format error / RCODE!=0 / truncated.
pub(crate) fn decode_answer_ips(msg: &[u8]) -> Result<Vec<IpAddr>> {
    if msg.len() < 12 {
        return Err(Error::Network(format!(
            "DNS: ответ короче header-а (12 байт): {} байт",
            msg.len()
        )));
    }
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    // RFC 1035 §4.1.1 — flags формат:
    //   QR(1) Opcode(4) AA(1) TC(1) RD(1) RA(1) Z(3) RCODE(4)
    let qr = (flags >> 15) & 1;
    if qr != 1 {
        return Err(Error::Network("DNS: ответ с QR=0 (это запрос)".to_owned()));
    }
    let tc = (flags >> 9) & 1;
    if tc != 0 {
        // Truncated — RFC 1035 §4.1.1; в DoH такого не должно быть
        // (HTTPS не ограничен 512 байтами UDP), но на всякий случай.
        return Err(Error::Network("DNS: TC=1, ответ обрезан".to_owned()));
    }
    let rcode = flags & 0x0F;
    if rcode != 0 {
        // 1=FormErr, 2=ServFail, 3=NXDOMAIN, 5=Refused …
        return Err(Error::Network(format!("DNS: RCODE={rcode}")));
    }
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let ancount = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    // NS и AR секции для нас не нужны — answer хранит конечные A/AAAA.

    let mut pos = 12;
    // Пропустить question section (qdcount раз: name + 4 байта qtype/qclass).
    for _ in 0..qdcount {
        pos = skip_name(msg, pos)?;
        if pos + 4 > msg.len() {
            return Err(Error::Network(
                "DNS: question section короче ожидаемого".to_owned(),
            ));
        }
        pos += 4;
    }
    // Answer section: ancount записей. Извлекаем RDATA для TYPE A/AAAA;
    // CNAME пропускаем (в RFC 1035 §3.4 сервер сам разворачивает цепочку
    // и кладёт финальный A/AAAA в той же answer-секции).
    let mut ips = Vec::new();
    for _ in 0..ancount {
        pos = skip_name(msg, pos)?;
        if pos + 10 > msg.len() {
            return Err(Error::Network(
                "DNS: answer header (type/class/ttl/rdlength) обрезан".to_owned(),
            ));
        }
        let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        // class в msg[pos+2..pos+4] — пропускаем (всегда IN=1 для интернет-имён)
        // ttl в msg[pos+4..pos+8] — игнорируем (см. doc — кеш сверху)
        let rdlength = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlength > msg.len() {
            return Err(Error::Network(format!(
                "DNS: RDATA выходит за пределы msg (pos={pos}, rdlength={rdlength}, len={})",
                msg.len()
            )));
        }
        match rtype {
            TYPE_A => {
                if rdlength != 4 {
                    return Err(Error::Network(format!(
                        "DNS: A record с RDLENGTH={rdlength}, ожидалось 4"
                    )));
                }
                let v = Ipv4Addr::new(msg[pos], msg[pos + 1], msg[pos + 2], msg[pos + 3]);
                ips.push(IpAddr::V4(v));
            }
            TYPE_AAAA => {
                if rdlength != 16 {
                    return Err(Error::Network(format!(
                        "DNS: AAAA record с RDLENGTH={rdlength}, ожидалось 16"
                    )));
                }
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&msg[pos..pos + 16]);
                ips.push(IpAddr::V6(Ipv6Addr::from(octets)));
            }
            // CNAME и прочее — пропускаем; финальные A/AAAA сервер
            // обычно кладёт в той же answer section. Без обработки CNAME
            // мы всё равно получим адреса в типичном ответе.
            _ => {}
        }
        pos += rdlength;
    }
    Ok(ips)
}

/// Пропустить domain name в произвольной позиции `pos` сообщения,
/// возвращая позицию ПОСЛЕ name. Поддерживает RFC 1035 §4.1.4
/// compression pointers (top two bits "11", remaining 14 bits = absolute
/// offset). Возвращаемая позиция — место в исходном указателе, где
/// продолжается чтение (не туда, куда «прыгает» pointer).
fn skip_name(msg: &[u8], mut pos: usize) -> Result<usize> {
    // RFC 1035 §2.3.4 — name не длиннее 255 байт; используем как hard cap.
    let mut traversed = 0;
    loop {
        if pos >= msg.len() {
            return Err(Error::Network(
                "DNS: name выходит за пределы сообщения".to_owned(),
            ));
        }
        let b = msg[pos];
        match b & 0xC0 {
            0x00 => {
                // Length byte (0..63). 0 = конец name.
                let len = b as usize;
                if len == 0 {
                    return Ok(pos + 1);
                }
                pos += 1 + len;
                traversed += 1 + len;
            }
            0xC0 => {
                // Pointer — 2 байта, мы их consume-им и сразу выходим;
                // фактический контент по pointer-у не разворачиваем (мы
                // только skip-аем, не извлекаем строку).
                if pos + 1 >= msg.len() {
                    return Err(Error::Network(
                        "DNS: pointer без второго байта".to_owned(),
                    ));
                }
                return Ok(pos + 2);
            }
            _ => {
                // 0x40 и 0x80 — reserved (extended label types, не используются).
                return Err(Error::Network(format!(
                    "DNS: reserved label type 0x{:02x} в позиции {pos}",
                    b & 0xC0
                )));
            }
        }
        if traversed > 255 {
            return Err(Error::Network(
                "DNS: name длиннее 255 байт без pointer-а".to_owned(),
            ));
        }
    }
}

// ── base64url (RFC 4648 §5) ──────────────────────────────────────────────────

/// Закодировать байты в base64url **без padding** — RFC 8484 §4.1 явно
/// требует `=`-padding опустить. Алфавит — стандартный base64 с заменой
/// `+`→`-` и `/`→`_`.
pub(crate) fn base64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = (u32::from(bytes[i]) << 16)
            | (u32::from(bytes[i + 1]) << 8)
            | u32::from(bytes[i + 2]);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHABET[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = u32::from(bytes[i]) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        // no padding
    } else if rem == 2 {
        let n = (u32::from(bytes[i]) << 16) | (u32::from(bytes[i + 1]) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
    }
    out
}

// ── DohResolver ──────────────────────────────────────────────────────────────

/// DNS-over-HTTPS резолвер.
///
/// Использует произвольный `NetworkTransport` (типично `HttpClient`)
/// для GET-запросов к DoH endpoint-у. `endpoint` — обычно
/// `https://<provider>/dns-query`; популярные провайдеры —
/// Cloudflare (`https://cloudflare-dns.com/dns-query`), Google
/// (`https://dns.google/dns-query`), Quad9 (`https://dns.quad9.net/dns-query`).
///
/// **Bootstrap.** `transport` сам должен уметь резолвить host endpoint-а —
/// если в endpoint-е DNS-имя (а не IP-литерал), внутренний `HttpClient`
/// должен использовать НЕ `DohResolver` (иначе бесконечная рекурсия).
/// Обычная схема:
/// ```ignore
/// let bootstrap = Arc::new(HttpClient::new()); // SystemDnsResolver
/// let doh = Arc::new(DohResolver::new(endpoint, bootstrap));
/// let main = HttpClient::new().with_dns_resolver(doh);
/// ```
/// Альтернатива — указать endpoint с IP-литералом в host
/// (`https://1.1.1.1/dns-query`), тогда DNS вообще не нужен; ценой
/// TLS server-name = "1.1.1.1" (некоторые серверы требуют SNI с
/// именем — Cloudflare принимает оба варианта).
pub struct DohResolver {
    endpoint: Url,
    transport: Arc<dyn NetworkTransport>,
}

impl DohResolver {
    /// `endpoint` — URL DoH сервера со схемой `https://`. `transport` —
    /// HTTP-клиент с подходящим bootstrap-резолвером (см. doc структуры).
    pub fn new(endpoint: Url, transport: Arc<dyn NetworkTransport>) -> Self {
        Self { endpoint, transport }
    }

    /// Отправить один DoH-запрос и распарсить ответ. Возвращает list
    /// IP-адресов или Err при wire / HTTP / RCODE-ошибке.
    fn query(&self, hostname: &str, qtype: u16) -> Result<Vec<IpAddr>> {
        // Wire query → base64url → URL.
        let wire = encode_query(0, hostname, qtype)?;
        let encoded = base64url_encode(&wire);
        let url = self.build_query_url(&encoded)?;
        // GET — transport.fetch проверяет статус 2xx и возвращает body.
        let body = self.transport.fetch(&url)?;
        decode_answer_ips(&body)
    }

    /// Построить URL с query-параметром `dns=...`. Если у endpoint-а
    /// уже есть query — добавляем через `&`, иначе через `?`. Не
    /// перезаписываем — пользователь мог указать что-то осмысленное
    /// (например, `?ct=application/dns-message` — некоторые legacy DoH-
    /// клиенты добавляли content-type-hint в query).
    fn build_query_url(&self, encoded: &str) -> Result<Url> {
        let base = self.endpoint.as_str();
        let sep = if self.endpoint.query().is_some() { '&' } else { '?' };
        // Fragment вряд ли осмыслен на DoH endpoint, но если есть —
        // его надо обрезать перед добавлением query (#... всегда
        // последний в URL). Простой случай: endpoint без fragment.
        let serialized = match base.find('#') {
            Some(i) => format!("{}{}dns={}{}", &base[..i], sep, encoded, &base[i..]),
            None => format!("{base}{sep}dns={encoded}"),
        };
        Url::parse(&serialized)
            .map_err(|e| Error::Network(format!("DoH: invalid query URL '{serialized}': {e}")))
    }
}

impl DnsResolver for DohResolver {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        // Литералы IP — не идут в DoH. Принимаем оба варианта IPv4/IPv6,
        // включая bracketed `[::1]` (на случай если caller передал
        // host-of-URL включая скобки — Url::host обычно их снимает, но
        // не больно подстраховаться).
        let unbracketed = hostname.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
        let literal_candidate = unbracketed.unwrap_or(hostname);
        if let Ok(ip) = IpAddr::from_str(literal_candidate) {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }

        // AAAA сначала (RFC 6724 §6 default — dual-stack: prefer IPv6
        // если он доступен), потом A. Если AAAA дал Err — продолжаем
        // на A; если A тоже Err — поднимаем последнюю ошибку. Если оба
        // вернули пусто — Err про NODATA (caller ждёт хотя бы один адрес).
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
                    "DoH: no addresses for {hostname} (NODATA from both A and AAAA)"
                ))
            }));
        }
        Ok(addrs)
    }
}

// ── Тесты ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TYPE_CNAME: u16 = 5;

    // ── wire encode ──

    #[test]
    fn encode_query_header_format() {
        let q = encode_query(0x1234, "example.com", TYPE_A).unwrap();
        // Header: ID, flags=0x0100, QDCOUNT=1, остальные 0.
        assert_eq!(&q[0..2], &[0x12, 0x34]);
        assert_eq!(&q[2..4], &[0x01, 0x00]);
        assert_eq!(&q[4..6], &[0x00, 0x01]); // QDCOUNT
        assert_eq!(&q[6..8], &[0x00, 0x00]); // ANCOUNT
        assert_eq!(&q[8..10], &[0x00, 0x00]); // NSCOUNT
        assert_eq!(&q[10..12], &[0x00, 0x00]); // ARCOUNT
    }

    #[test]
    fn encode_query_qname_labels() {
        let q = encode_query(0, "example.com", TYPE_A).unwrap();
        // После header (12 байт): 7 'example' 3 'com' 0
        assert_eq!(&q[12..], &[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
            3, b'c', b'o', b'm',
            0,
            0x00, 0x01, // QTYPE = A
            0x00, 0x01, // QCLASS = IN
        ]);
    }

    #[test]
    fn encode_query_trailing_dot_ok() {
        // "example.com." (с trailing dot) и "example.com" должны давать
        // одинаковый wire-формат.
        let q1 = encode_query(0, "example.com", TYPE_A).unwrap();
        let q2 = encode_query(0, "example.com.", TYPE_A).unwrap();
        assert_eq!(q1, q2);
    }

    #[test]
    fn encode_query_root_domain() {
        let q = encode_query(0, "", TYPE_A).unwrap();
        // Header + 1 null label + 4 байта qtype/qclass
        assert_eq!(q.len(), 12 + 1 + 4);
        assert_eq!(q[12], 0);
    }

    #[test]
    fn encode_query_punycode_label() {
        // IDN-домены должны быть уже в Punycode (caller отвечает за это).
        let q = encode_query(0, "xn--80a1acny.xn--p1ai", TYPE_A).unwrap();
        // ничего магического — обычные labels, проверяем что не упало
        // и есть оба null-terminator-ы (по одному per label, плюс root).
        assert!(q.iter().filter(|&&b| b == 0).count() >= 1);
    }

    #[test]
    fn encode_query_label_too_long_errors() {
        let long = "a".repeat(64);
        assert!(encode_query(0, &long, TYPE_A).is_err());
    }

    #[test]
    fn encode_query_consecutive_dots_error() {
        // ".." → пустой label между точками.
        assert!(encode_query(0, "foo..bar", TYPE_A).is_err());
    }

    #[test]
    fn encode_query_aaaa_qtype() {
        let q = encode_query(0, "example.com", TYPE_AAAA).unwrap();
        assert_eq!(&q[q.len() - 4..q.len() - 2], &[0x00, 0x1C]); // TYPE_AAAA = 28
    }

    // ── wire decode ──

    /// Собрать ответ: header c QDCOUNT=1, ANCOUNT=ancount; question
    /// "example.com IN A"; затем `answer_section` дописывается as-is.
    fn build_response(flags: u16, ancount: u16, answer_section: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&0u16.to_be_bytes()); // ID
        msg.extend_from_slice(&flags.to_be_bytes());
        msg.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
        msg.extend_from_slice(&ancount.to_be_bytes()); // ANCOUNT
        msg.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
        msg.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
        // Question: example.com IN A
        msg.extend_from_slice(&[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
            3, b'c', b'o', b'm',
            0,
            0x00, 0x01, 0x00, 0x01,
        ]);
        msg.extend_from_slice(answer_section);
        msg
    }

    /// Answer record c name через pointer на question (offset 12).
    fn a_record_via_pointer(ip: [u8; 4]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&[0xC0, 0x0C]); // pointer на question name (offset 12)
        a.extend_from_slice(&TYPE_A.to_be_bytes());
        a.extend_from_slice(&CLASS_IN.to_be_bytes());
        a.extend_from_slice(&60u32.to_be_bytes()); // TTL
        a.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        a.extend_from_slice(&ip);
        a
    }

    fn aaaa_record_via_pointer(ip: [u8; 16]) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(&[0xC0, 0x0C]);
        a.extend_from_slice(&TYPE_AAAA.to_be_bytes());
        a.extend_from_slice(&CLASS_IN.to_be_bytes());
        a.extend_from_slice(&60u32.to_be_bytes());
        a.extend_from_slice(&16u16.to_be_bytes());
        a.extend_from_slice(&ip);
        a
    }

    #[test]
    fn decode_single_a_record() {
        // QR=1, RD=1, RA=1, RCODE=0 → flags = 0x8180.
        let msg = build_response(0x8180, 1, &a_record_via_pointer([93, 184, 216, 34]));
        let ips = decode_answer_ips(&msg).unwrap();
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]);
    }

    #[test]
    fn decode_single_aaaa_record() {
        // 2606:2800:220:1:248:1893:25c8:1946 (пример из example.com IANA).
        let ip = [
            0x26, 0x06, 0x28, 0x00, 0x02, 0x20, 0x00, 0x01,
            0x02, 0x48, 0x18, 0x93, 0x25, 0xC8, 0x19, 0x46,
        ];
        let msg = build_response(0x8180, 1, &aaaa_record_via_pointer(ip));
        let ips = decode_answer_ips(&msg).unwrap();
        let want: Ipv6Addr = "2606:2800:220:1:248:1893:25c8:1946".parse().unwrap();
        assert_eq!(ips, vec![IpAddr::V6(want)]);
    }

    #[test]
    fn decode_multiple_a_records_in_one_answer() {
        let mut answers = Vec::new();
        answers.extend_from_slice(&a_record_via_pointer([1, 2, 3, 4]));
        answers.extend_from_slice(&a_record_via_pointer([5, 6, 7, 8]));
        let msg = build_response(0x8180, 2, &answers);
        let ips = decode_answer_ips(&msg).unwrap();
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0], IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        assert_eq!(ips[1], IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8)));
    }

    #[test]
    fn decode_cname_record_skipped_a_record_returned() {
        // Two answers: CNAME (skipped) + A (returned).
        let mut cname = Vec::new();
        cname.extend_from_slice(&[0xC0, 0x0C]);
        cname.extend_from_slice(&TYPE_CNAME.to_be_bytes());
        cname.extend_from_slice(&CLASS_IN.to_be_bytes());
        cname.extend_from_slice(&60u32.to_be_bytes());
        // RDATA = "target.example.com" + null
        let target = b"\x06target\x07example\x03com\x00";
        cname.extend_from_slice(&(target.len() as u16).to_be_bytes());
        cname.extend_from_slice(target);

        let mut all = cname;
        all.extend_from_slice(&a_record_via_pointer([10, 20, 30, 40]));
        let msg = build_response(0x8180, 2, &all);
        let ips = decode_answer_ips(&msg).unwrap();
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40))]);
    }

    #[test]
    fn decode_nxdomain_returns_err() {
        // RCODE=3 (NXDOMAIN)
        let msg = build_response(0x8183, 0, &[]);
        let err = decode_answer_ips(&msg).unwrap_err();
        let s = format!("{err}");
        assert!(s.contains("RCODE=3"), "{s}");
    }

    #[test]
    fn decode_servfail_returns_err() {
        let msg = build_response(0x8182, 0, &[]);
        assert!(decode_answer_ips(&msg).is_err());
    }

    #[test]
    fn decode_truncated_message_errors() {
        // Header указывает QR=1, RCODE=0, но обрезано.
        let msg = vec![0, 0, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0]; // 11 байт
        assert!(decode_answer_ips(&msg).is_err());
    }

    #[test]
    fn decode_qr_zero_is_request_not_response() {
        let mut msg = build_response(0x0100, 0, &[]); // QR=0
        // Заодно очистим answer count
        msg[6] = 0;
        msg[7] = 0;
        assert!(decode_answer_ips(&msg).is_err());
    }

    #[test]
    fn decode_tc_set_returns_err() {
        // TC=1 (bit 9)
        let msg = build_response(0x8380, 0, &[]);
        assert!(decode_answer_ips(&msg).is_err());
    }

    #[test]
    fn decode_no_answers_returns_empty_vec() {
        let msg = build_response(0x8180, 0, &[]);
        let ips = decode_answer_ips(&msg).unwrap();
        assert!(ips.is_empty());
    }

    #[test]
    fn decode_a_with_wrong_rdlength_errors() {
        let mut bad = Vec::new();
        bad.extend_from_slice(&[0xC0, 0x0C]);
        bad.extend_from_slice(&TYPE_A.to_be_bytes());
        bad.extend_from_slice(&CLASS_IN.to_be_bytes());
        bad.extend_from_slice(&60u32.to_be_bytes());
        bad.extend_from_slice(&5u16.to_be_bytes()); // RDLENGTH=5 (должно быть 4)
        bad.extend_from_slice(&[1, 2, 3, 4, 5]);
        let msg = build_response(0x8180, 1, &bad);
        assert!(decode_answer_ips(&msg).is_err());
    }

    #[test]
    fn decode_name_with_inline_labels_then_terminator() {
        // Inline (без pointer) — name закодирован прямо в answer как
        // "example.com" с null-terminator-ом.
        let mut a = Vec::new();
        a.extend_from_slice(&[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e']);
        a.extend_from_slice(&[3, b'c', b'o', b'm', 0]);
        a.extend_from_slice(&TYPE_A.to_be_bytes());
        a.extend_from_slice(&CLASS_IN.to_be_bytes());
        a.extend_from_slice(&60u32.to_be_bytes());
        a.extend_from_slice(&4u16.to_be_bytes());
        a.extend_from_slice(&[127, 0, 0, 1]);
        let msg = build_response(0x8180, 1, &a);
        let ips = decode_answer_ips(&msg).unwrap();
        assert_eq!(ips, vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]);
    }

    // ── base64url ──

    #[test]
    fn base64url_empty() {
        assert_eq!(base64url_encode(&[]), "");
    }

    #[test]
    fn base64url_one_byte() {
        // RFC 4648 §10: "f" → "Zg==" → no padding → "Zg".
        assert_eq!(base64url_encode(b"f"), "Zg");
    }

    #[test]
    fn base64url_two_bytes() {
        // "fo" → "Zm8=" → "Zm8"
        assert_eq!(base64url_encode(b"fo"), "Zm8");
    }

    #[test]
    fn base64url_three_bytes() {
        // "foo" → "Zm9v"
        assert_eq!(base64url_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn base64url_long_string() {
        assert_eq!(base64url_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_url_safe_chars() {
        // Байты, которые в обычном base64 дают `+` или `/`.
        // 0xfb 0xff 0xfe → b64 "+//+" → b64url "-__-"
        assert_eq!(base64url_encode(&[0xfb, 0xff, 0xfe]), "-__-");
    }

    // ── DohResolver через mock transport ──

    /// Mock `NetworkTransport`, отдающий заранее заготовленные ответы по
    /// порядку. Каждый fetch фиксирует переданный URL для проверки
    /// формата DoH-запроса (base64url, &/? разделитель и т.д.).
    struct MockTransport {
        responses: Mutex<Vec<Result<Vec<u8>>>>,
        urls: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<Vec<u8>>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                urls: Mutex::new(Vec::new()),
            }
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    impl NetworkTransport for MockTransport {
        fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
            self.urls.lock().unwrap().push(url.as_str().to_owned());
            let mut r = self.responses.lock().unwrap();
            if r.is_empty() {
                return Err(Error::Network("mock: no more responses".to_owned()));
            }
            r.remove(0)
        }
    }

    fn mock_doh(responses: Vec<Result<Vec<u8>>>) -> (DohResolver, Arc<MockTransport>) {
        let transport = Arc::new(MockTransport::new(responses));
        let endpoint = Url::parse("https://example-dns.test/dns-query").unwrap();
        let resolver = DohResolver::new(endpoint, transport.clone());
        (resolver, transport)
    }

    #[test]
    fn resolve_returns_combined_aaaa_and_a() {
        // Первый ответ — AAAA, второй — A. AAAA должен идти первым (RFC 6724).
        let aaaa_ip = [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        let aaaa_resp = build_response(0x8180, 1, &aaaa_record_via_pointer(aaaa_ip));
        let a_resp = build_response(0x8180, 1, &a_record_via_pointer([192, 0, 2, 1]));
        let (resolver, _t) = mock_doh(vec![Ok(aaaa_resp), Ok(a_resp)]);
        let addrs = resolver.resolve("example.com", 443).unwrap();
        assert_eq!(addrs.len(), 2);
        let v6: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert_eq!(addrs[0], SocketAddr::new(IpAddr::V6(v6), 443));
        assert_eq!(
            addrs[1],
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 443)
        );
    }

    #[test]
    fn resolve_ipv4_only_domain_works() {
        // AAAA вернул пустой ответ — нормальная ситуация для legacy-домена.
        let aaaa_empty = build_response(0x8180, 0, &[]);
        let a_resp = build_response(0x8180, 1, &a_record_via_pointer([10, 0, 0, 1]));
        let (resolver, _t) = mock_doh(vec![Ok(aaaa_empty), Ok(a_resp)]);
        let addrs = resolver.resolve("ipv4only.test", 80).unwrap();
        assert_eq!(addrs, vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 80)]);
    }

    #[test]
    fn resolve_ipv6_only_domain_works() {
        let aaaa_resp = build_response(
            0x8180,
            1,
            &aaaa_record_via_pointer([
                0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
            ]),
        );
        let a_empty = build_response(0x8180, 0, &[]);
        let (resolver, _t) = mock_doh(vec![Ok(aaaa_resp), Ok(a_empty)]);
        let addrs = resolver.resolve("ipv6only.test", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
    }

    #[test]
    fn resolve_both_empty_returns_err() {
        let aaaa_empty = build_response(0x8180, 0, &[]);
        let a_empty = build_response(0x8180, 0, &[]);
        let (resolver, _t) = mock_doh(vec![Ok(aaaa_empty), Ok(a_empty)]);
        let err = resolver.resolve("nodata.test", 80).unwrap_err();
        assert!(format!("{err}").contains("no addresses"));
    }

    #[test]
    fn resolve_nxdomain_for_both_returns_err() {
        // RCODE=3 → NXDOMAIN.
        let nxd1 = build_response(0x8183, 0, &[]);
        let nxd2 = build_response(0x8183, 0, &[]);
        let (resolver, _t) = mock_doh(vec![Ok(nxd1), Ok(nxd2)]);
        let err = resolver.resolve("nodomain.test", 80).unwrap_err();
        assert!(format!("{err}").contains("RCODE=3"));
    }

    #[test]
    fn resolve_aaaa_fails_a_ok_returns_a() {
        // AAAA — RCODE=2 (SERVFAIL), A — нормальный. Должен вернуть A.
        let aaaa_fail = build_response(0x8182, 0, &[]);
        let a_resp = build_response(0x8180, 1, &a_record_via_pointer([7, 7, 7, 7]));
        let (resolver, _t) = mock_doh(vec![Ok(aaaa_fail), Ok(a_resp)]);
        let addrs = resolver.resolve("partial.test", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].ip(), IpAddr::V4(Ipv4Addr::new(7, 7, 7, 7)));
    }

    #[test]
    fn resolve_ipv4_literal_bypasses_doh() {
        // Никаких ответов в очереди — если ходило бы в transport, упало бы.
        let (resolver, t) = mock_doh(vec![]);
        let addrs = resolver.resolve("8.8.8.8", 53).unwrap();
        assert_eq!(addrs, vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53)]);
        assert!(t.urls().is_empty(), "transport должен НЕ вызываться для IP-литерала");
    }

    #[test]
    fn resolve_ipv6_literal_bypasses_doh() {
        let (resolver, _t) = mock_doh(vec![]);
        let addrs = resolver.resolve("::1", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
    }

    #[test]
    fn resolve_bracketed_ipv6_literal_bypasses_doh() {
        let (resolver, _t) = mock_doh(vec![]);
        let addrs = resolver.resolve("[::1]", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
    }

    #[test]
    fn resolve_uses_get_with_dns_query_param() {
        let aaaa_empty = build_response(0x8180, 0, &[]);
        let a_resp = build_response(0x8180, 1, &a_record_via_pointer([1, 1, 1, 1]));
        let (resolver, t) = mock_doh(vec![Ok(aaaa_empty), Ok(a_resp)]);
        resolver.resolve("example.com", 443).unwrap();
        let urls = t.urls();
        assert_eq!(urls.len(), 2);
        for u in &urls {
            assert!(u.starts_with("https://example-dns.test/dns-query?dns="), "{u}");
            // base64url alphabet — никаких '+' '/' '=' в URL.
            let q = u.split("dns=").nth(1).unwrap();
            assert!(!q.contains('+'));
            assert!(!q.contains('/'));
            assert!(!q.contains('='));
        }
    }

    #[test]
    fn build_query_url_appends_with_ampersand_if_endpoint_has_query() {
        let endpoint = Url::parse("https://example-dns.test/dns-query?ct=app").unwrap();
        let transport = Arc::new(MockTransport::new(vec![]));
        let resolver = DohResolver::new(endpoint, transport);
        let url = resolver.build_query_url("AABB").unwrap();
        assert_eq!(
            url.as_str(),
            "https://example-dns.test/dns-query?ct=app&dns=AABB"
        );
    }

    #[test]
    fn doh_resolver_is_send_sync_object_safe() {
        fn check<T: Send + Sync>() {}
        check::<DohResolver>();
        // Object-safety: DohResolver можно положить в Arc<dyn DnsResolver>.
        let transport = Arc::new(MockTransport::new(vec![]));
        let endpoint = Url::parse("https://example-dns.test/dns-query").unwrap();
        let resolver: Arc<dyn DnsResolver> = Arc::new(DohResolver::new(endpoint, transport));
        let _ = resolver;
    }
}
