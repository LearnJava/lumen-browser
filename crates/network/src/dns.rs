//! DNS-резолверы: системный (default) и интеграционная точка для подмены.
//!
//! `lumen_core::ext::DnsResolver` — публичный trait; `SystemDnsResolver`
//! ниже — единственная реализация, живущая в lumen-network (через
//! `(host, port).to_socket_addrs()` из std). Кешированные / DoH / DoT
//! реализации — отдельные crate-ы (`lumen-storage::CachedDnsResolver`),
//! lumen-network знает их только через trait.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use lumen_core::error::{Error, Result};
use lumen_core::ext::DnsResolver;

/// Переменная окружения с переопределением DNS: `host=ip[,host=ip…]`, по
/// образцу `curl --resolve` (WPT-LUMEN-RESOLVE).
///
/// Нужна раннеру WPT: апстримный `browser_host: web-platform.test` и
/// альтернативный `not-web-platform.test` с поддоменами не резолвятся без
/// правки `/etc/hosts`, а правка системного файла — не то, что раннер вправе
/// делать. Читается один раз на процесс; не задана — резолв байт-в-байт
/// прежний. Имена — ASCII без учёта регистра (IDN — в punycode, как его
/// отдаёт URL-парсер); повтор имени добавляет адрес. Битая пара — диагностика
/// в stderr и пропуск, как у `LUMEN_EXTRA_CA_CERT`.
pub const RESOLVE_ENV: &str = "LUMEN_RESOLVE";

/// Карта переопределений: нормализованное имя → адреса в порядке задания.
type ResolveMap = HashMap<String, Vec<IpAddr>>;

/// Нормализованная форма имени для поиска в карте: без хвостовой точки и
/// IPv6-скобок, ASCII-нижний регистр.
fn normalize_host(hostname: &str) -> String {
    let host = hostname.strip_suffix('.').unwrap_or(hostname);
    let host = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(host);
    host.to_ascii_lowercase()
}

/// Разобрать `host=ip[,host=ip…]`. Пустые сегменты пропускаются молча,
/// битые пары (нет `=`, пустое или не-ASCII имя, имя с `:`/пробелом, не-IP
/// значение) — с диагностикой в stderr.
fn parse_resolve_spec(spec: &str) -> ResolveMap {
    let mut map = ResolveMap::new();
    for pair in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((name, ip)) = pair.split_once('=') else {
            eprintln!("{RESOLVE_ENV}: пара `{pair}` без `=`, пропускаю");
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || !name.is_ascii()
            || name.contains(|c: char| c == ':' || c == '/' || c.is_ascii_whitespace())
        {
            eprintln!("{RESOLVE_ENV}: имя `{name}` в паре `{pair}` не ASCII-хост, пропускаю");
            continue;
        }
        let ip = ip.trim();
        let ip_unbracketed = ip.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(ip);
        let Ok(ip) = IpAddr::from_str(ip_unbracketed) else {
            eprintln!("{RESOLVE_ENV}: `{ip}` в паре `{pair}` не IP-адрес, пропускаю");
            continue;
        };
        let addrs = map.entry(normalize_host(name)).or_default();
        if !addrs.contains(&ip) {
            addrs.push(ip);
        }
    }
    map
}

/// Карта из [`RESOLVE_ENV`], разобранная один раз на процесс.
fn env_resolve_map() -> &'static ResolveMap {
    static MAP: OnceLock<ResolveMap> = OnceLock::new();
    MAP.get_or_init(|| std::env::var(RESOLVE_ENV).map(|s| parse_resolve_spec(&s)).unwrap_or_default())
}

/// Адреса из карты для `hostname` с подставленным `port`; `None` — имени в
/// карте нет (пустая карта отвечает без нормализации имени).
fn lookup(map: &ResolveMap, hostname: &str, port: u16) -> Option<Vec<SocketAddr>> {
    if map.is_empty() {
        return None;
    }
    map.get(&normalize_host(hostname))
        .map(|ips| ips.iter().map(|&ip| SocketAddr::new(ip, port)).collect())
}

/// Резолв с учётом [`RESOLVE_ENV`]: имя из карты отвечает её адресами, иначе
/// — `resolver`. Через эту функцию идут все соединения (`connect_inner` —
/// прямое и SOCKS5-прокси, `h3::client_transport` — QUIC), поэтому
/// переопределение действует поверх любого резолвера: системного,
/// `CachedDnsResolver`, DoH, DoT.
pub(crate) fn resolve_with_env_override(
    resolver: &dyn DnsResolver,
    hostname: &str,
    port: u16,
) -> Result<Vec<SocketAddr>> {
    match lookup(env_resolve_map(), hostname, port) {
        Some(addrs) => Ok(addrs),
        None => resolver.resolve(hostname, port),
    }
}

/// Резолвер-обёртка с явной картой переопределений (синтаксис
/// [`RESOLVE_ENV`]): имя из карты → её адреса с запрошенным портом, промах —
/// во `inner`. Для встраивающего кода и тестов, которым нужна карта, не
/// зависящая от окружения процесса; переменная окружения применяется в
/// `HttpClient` и без этой обёртки.
pub struct ResolveOverride {
    map: ResolveMap,
    inner: Arc<dyn DnsResolver>,
}

impl ResolveOverride {
    /// Обёртка над `inner` с картой из строки `host=ip[,host=ip…]`.
    pub fn new(spec: &str, inner: Arc<dyn DnsResolver>) -> Self {
        Self { map: parse_resolve_spec(spec), inner }
    }
}

impl DnsResolver for ResolveOverride {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        match lookup(&self.map, hostname, port) {
            Some(addrs) => Ok(addrs),
            None => self.inner.resolve(hostname, port),
        }
    }
}

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

/// `localhost` и `*.localhost` (регистронезависимо, допустима хвостовая точка)
/// → loopback без обращения к резолверу (RFC 6761 §6.3, BUG-1070). Windows
/// `getaddrinfo` разрешает только сам `localhost` из hosts-файла, поддомены
/// (`www.localhost`, `dev.app.localhost`) отдаёт как WSAHOST_NOT_FOUND.
/// Порядок: IPv4 первым — dev-серверы чаще слушают только `127.0.0.1`.
pub(crate) fn localhost_addrs(hostname: &str, port: u16) -> Option<Vec<SocketAddr>> {
    let host = hostname.strip_suffix('.').unwrap_or(hostname).to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        Some(vec![
            SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            SocketAddr::new(IpAddr::from(std::net::Ipv6Addr::LOCALHOST), port),
        ])
    } else {
        None
    }
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
        if let Some(addrs) = localhost_addrs(hostname, port) {
            return Ok(addrs);
        }
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
    fn localhost_subdomains_resolve_to_loopback() {
        for h in ["www.localhost", "WWW1.Localhost", "dev.app.localhost.", "localhost."] {
            let addrs = SystemDnsResolver.resolve(h, 18300).unwrap();
            assert!(addrs.iter().all(|a| a.ip().is_loopback() && a.port() == 18300), "{h}");
            assert!(addrs[0].is_ipv4(), "{h}");
        }
        assert!(localhost_addrs("notlocalhost", 1).is_none());
        assert!(localhost_addrs("localhost.example", 1).is_none());
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

    /// Резолвер-заглушка: любое имя → `10.9.9.9`, чтобы промах карты был
    /// виден без сети.
    struct FixedResolver;

    impl DnsResolver for FixedResolver {
        fn resolve(&self, _hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
            Ok(vec![SocketAddr::new(IpAddr::from([10, 9, 9, 9]), port)])
        }
    }

    #[test]
    fn resolve_spec_parses_v4_and_v6() {
        let map = parse_resolve_spec("a.test=127.0.0.1,b.test=::1");
        assert_eq!(map["a.test"], vec![IpAddr::from([127, 0, 0, 1])]);
        assert_eq!(map["b.test"], vec![IpAddr::from(std::net::Ipv6Addr::LOCALHOST)]);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn resolve_spec_repeated_name_accumulates_and_dedups() {
        let map = parse_resolve_spec(" a.test = 127.0.0.1 , A.test=[::1],a.test=127.0.0.1,");
        assert_eq!(
            map["a.test"],
            vec![IpAddr::from([127, 0, 0, 1]), IpAddr::from(std::net::Ipv6Addr::LOCALHOST)]
        );
    }

    #[test]
    fn resolve_spec_skips_broken_pairs() {
        let map = parse_resolve_spec(
            "noeq,=127.0.0.1,bad.test=999.1.1.1,host:80=127.0.0.1,пример.test=127.0.0.1,ok.test=10.0.0.1",
        );
        assert_eq!(map.len(), 1, "{map:?}");
        assert_eq!(map["ok.test"], vec![IpAddr::from([10, 0, 0, 1])]);
        assert!(parse_resolve_spec("").is_empty());
    }

    #[test]
    fn override_is_case_insensitive_and_substitutes_port() {
        let r = ResolveOverride::new("Web-Platform.TEST=127.0.0.1,web-platform.test=::1", Arc::new(FixedResolver));
        for host in ["web-platform.test", "WEB-PLATFORM.test", "web-platform.test."] {
            let addrs = r.resolve(host, 8443).unwrap();
            assert_eq!(
                addrs,
                vec!["127.0.0.1:8443".parse().unwrap(), "[::1]:8443".parse().unwrap()],
                "{host}"
            );
        }
    }

    #[test]
    fn override_miss_goes_to_inner() {
        let r = ResolveOverride::new("a.test=127.0.0.1", Arc::new(FixedResolver));
        assert_eq!(r.resolve("b.test", 80).unwrap(), vec!["10.9.9.9:80".parse().unwrap()]);
        // Поддомен — отдельное имя: wildcard-ов в карте нет, как у curl.
        assert_eq!(r.resolve("www.a.test", 80).unwrap(), vec!["10.9.9.9:80".parse().unwrap()]);
        let empty = ResolveOverride::new("", Arc::new(FixedResolver));
        assert_eq!(empty.resolve("a.test", 81).unwrap(), vec!["10.9.9.9:81".parse().unwrap()]);
    }
}
