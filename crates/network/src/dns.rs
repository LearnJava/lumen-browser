//! DNS-резолверы: системный (default) и интеграционная точка для подмены.
//!
//! `lumen_core::ext::DnsResolver` — публичный trait; `SystemDnsResolver`
//! ниже — единственная реализация, живущая в lumen-network (через
//! `(host, port).to_socket_addrs()` из std). Кешированные / DoH / DoT
//! реализации — отдельные crate-ы (`lumen-storage::CachedDnsResolver`),
//! lumen-network знает их только через trait.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use lumen_core::error::{Error, Result};
use lumen_core::ext::DnsResolver;

/// Код WSANO_DATA («имя верно, но записей запрошенного типа нет») из Winsock.
/// Windows-getaddrinfo отдаёт его в том числе на sinkhole-ответ `0.0.0.0`/`::`
/// от блокирующего DNS — сырой текст ошибки при этом ничего не объясняет
/// (BUG-304), поэтому к нему дописывается подсказка.
#[cfg(windows)]
const WSANO_DATA: i32 = 11004;

/// Отбросить sinkhole-адреса (`0.0.0.0` / `::`) из ответа резолвера.
///
/// Блокировщики отдают именно unspecified-адрес: hosts-файлы формата
/// `0.0.0.0 tracker.example`, AdGuard-подобные DNS-серверы. Как цель
/// соединения он не значит «этот хост» — `TcpStream::connect("0.0.0.0:443")`
/// уходит на локальную машину, то есть запрос к заблокированному домену
/// молча ушёл бы на сервис пользователя на том же порту. Windows-getaddrinfo
/// такие ответы отсеивает сам (WSANO_DATA), Linux/macOS — нет, а DoH/DoT
/// разбирают записи самостоятельно и системного фильтра не видят вовсе
/// (BUG-304).
///
/// Loopback (`127.0.0.1`) намеренно НЕ фильтруется: это валидный ответ для
/// локальной разработки (`*.localtest.me` и т.п.).
///
/// Пустой результат при непустом входе — Err с явной причиной; пустой вход
/// проходит насквозь (у вызывающих своя ошибка на «адресов нет вообще»).
pub(crate) fn reject_sinkhole_addrs(
    hostname: &str,
    addrs: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>> {
    let had_any = !addrs.is_empty();
    let usable: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|a| !a.ip().is_unspecified())
        .collect();
    if had_any && usable.is_empty() {
        return Err(Error::Network(format!(
            "resolve {hostname}: DNS answered with a sinkhole address only \
             (0.0.0.0/::) — host is blocked by the DNS server or hosts file"
        )));
    }
    Ok(usable)
}

/// Является ли hostname IP-литералом (в т.ч. в скобках, `[::1]`).
///
/// Литерал — явное намерение пользователя (`http://0.0.0.0:8080/` — обычный
/// адрес локального dev-сервера), поэтому sinkhole-фильтр к нему не
/// применяется; DoH/DoT по той же причине обрабатывают литералы до запроса.
fn is_ip_literal(hostname: &str) -> bool {
    let unbracketed = hostname.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
    IpAddr::from_str(unbracketed.unwrap_or(hostname)).is_ok()
}

/// Подсказка к сырой ошибке getaddrinfo, если это WSANO_DATA.
#[cfg(windows)]
fn nodata_hint(e: &std::io::Error) -> &'static str {
    if e.raw_os_error() == Some(WSANO_DATA) {
        " (name exists but has no address records — usually DNS-level blocking)"
    } else {
        ""
    }
}

/// На не-Windows подсказки нет: getaddrinfo там отдаёт sinkhole-адрес как
/// обычный ответ, и его отбрасывает `reject_sinkhole_addrs`.
#[cfg(not(windows))]
fn nodata_hint(_e: &std::io::Error) -> &'static str {
    ""
}

/// DNS-резолвер на основе системного getaddrinfo (через std::net).
///
/// Default-резолвер для `HttpClient` — поведение совпадает с прежним
/// `TcpStream::connect("host:port")`, который внутренне делает то же
/// самое. Пустой результат от `to_socket_addrs` маппится в Err — не в
/// пустой Vec, потому что для системного резолвера «нет адресов» это
/// аномалия (NXDOMAIN обычно возвращается как io::ErrorKind::NotFound).
/// Sinkhole-ответы блокировщиков (`0.0.0.0`/`::`) отбрасываются, см.
/// [`reject_sinkhole_addrs`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let target = (hostname, port);
        let resolved: Vec<SocketAddr> = target
            .to_socket_addrs()
            .map_err(|e| Error::Network(format!("resolve {hostname}: {e}{}", nodata_hint(&e))))?
            .collect();
        let addrs = if is_ip_literal(hostname) {
            resolved
        } else {
            reject_sinkhole_addrs(hostname, resolved)?
        };
        if addrs.is_empty() {
            return Err(Error::Network(format!(
                "resolve {hostname}: no addresses returned"
            )));
        }
        Ok(addrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_resolver_localhost() {
        // Через /etc/hosts (POSIX) и hosts-файл на Windows — всегда работает,
        // не делает реального DNS-вызова. Самый стабильный тест для интеграции
        // SystemDnsResolver с реальной системой.
        let addrs = SystemDnsResolver.resolve("localhost", 8080).unwrap();
        assert!(!addrs.is_empty());
        assert!(addrs.iter().all(|a| a.port() == 8080));
        assert!(addrs.iter().any(|a| a.ip().is_loopback()));
    }

    #[test]
    fn system_resolver_literal_ip_v4() {
        // Литеральный IP-адрес не должен идти в DNS вообще — getaddrinfo
        // отдаёт его в Vec<SocketAddr> as-is.
        let addrs = SystemDnsResolver.resolve("127.0.0.1", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].to_string(), "127.0.0.1:443");
    }

    #[test]
    fn system_resolver_literal_ip_v6() {
        let addrs = SystemDnsResolver.resolve("::1", 443).unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].port(), 443);
        assert!(addrs[0].ip().is_loopback());
    }

    #[test]
    fn sinkhole_only_answer_is_rejected() {
        // Ответ блокировщика: 0.0.0.0 и/или ::. Пригодных адресов не остаётся
        // → Err с явной причиной, а не connect на локальную машину (BUG-304).
        let addrs = vec![
            "0.0.0.0:443".parse().unwrap(),
            "[::]:443".parse().unwrap(),
        ];
        let err = reject_sinkhole_addrs("blocked.test", addrs).unwrap_err();
        assert!(format!("{err}").contains("sinkhole"), "получено: {err}");
    }

    #[test]
    fn sinkhole_mixed_answer_keeps_usable_addrs() {
        let addrs = vec![
            "0.0.0.0:443".parse().unwrap(),
            "93.184.216.34:443".parse().unwrap(),
        ];
        let kept = reject_sinkhole_addrs("partly.test", addrs).unwrap();
        assert_eq!(kept, vec!["93.184.216.34:443".parse().unwrap()]);
    }

    #[test]
    fn sinkhole_filter_keeps_loopback() {
        // 127.0.0.1 — валидный ответ (локальная разработка), не блокировка.
        let addrs = vec!["127.0.0.1:8080".parse().unwrap()];
        let kept = reject_sinkhole_addrs("localtest.test", addrs).unwrap();
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn sinkhole_filter_passes_empty_input_through() {
        // Пустой вход — не «заблокировано»; своя ошибка у вызывающего.
        let kept = reject_sinkhole_addrs("nodata.test", Vec::new()).unwrap();
        assert!(kept.is_empty());
    }

    #[test]
    fn system_resolver_unspecified_literal_passes() {
        // http://0.0.0.0:8080/ — обычный адрес локального dev-сервера;
        // литерал не должен попадать под sinkhole-фильтр.
        let addrs = SystemDnsResolver.resolve("0.0.0.0", 8080).unwrap();
        assert_eq!(addrs, vec!["0.0.0.0:8080".parse().unwrap()]);
    }

    #[test]
    fn ip_literal_detection() {
        assert!(is_ip_literal("127.0.0.1"));
        assert!(is_ip_literal("::1"));
        assert!(is_ip_literal("[::1]"));
        assert!(!is_ip_literal("example.com"));
    }

    #[test]
    fn system_resolver_is_send_sync_object_safe() {
        fn check<T: Send + Sync>() {}
        check::<SystemDnsResolver>();
        // Object-safety: можно положить в Box<dyn DnsResolver>.
        let _: Box<dyn DnsResolver> = Box::new(SystemDnsResolver);
    }
}
