//! `CachedDnsResolver` — обёртка вокруг произвольного [`DnsResolver`]
//! (системного, DoH, mock), которая держит TTL-кеш поверх [`DnsCache`].
//!
//! Живёт в lumen-storage, а не в lumen-network, потому что зависит от
//! `DnsCache` (SQLite). lumen-network знает её только через trait
//! `lumen_core::ext::DnsResolver` — никакого обратного dep-направления
//! storage → network нет.
//!
//! Поведение на каждый `resolve(host, port)`:
//! 1. `cache.get(host, now)` — если fresh-запись с непустым `addresses`
//!    есть, парсим строки в `IpAddr` и собираем `SocketAddr` с тем же
//!    `port`. Битые строки или невалидные IP-формы тихо отбрасываются
//!    (corrupt-cache → fallback); если в итоге список пуст — идём в
//!    inner-резолвер.
//! 2. Cache miss / expired / corrupt → `inner.resolve(host, port)`, затем
//!    `cache.put(host, addr_strings, now, default_ttl_seconds)` —
//!    кэшируем результат, чтобы следующий fetch к тому же origin не
//!    дёргал DNS снова. Пустой результат от inner в кэш не пишется
//!    (negative-caching отложен — не хочется ловить эфемерный network
//!    blip на 5 минут).
//!
//! TTL фиксированный (`default_ttl_seconds`), не из реального DNS-ответа
//! — std::net не отдаёт TTL, а DoH/DoT не реализованы. 300 c —
//! разумный default между «не нагружать резолвер» и «не пропустить
//! быстрый failover».

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use lumen_core::error::Result;
use lumen_core::ext::DnsResolver;

use crate::dns_cache::DnsCache;

/// Источник unix-времени. Дефолт — `SystemTime::now` через
/// [`SystemClock`]; тесты подменяют на `MockClock`, чтобы детерминированно
/// проверять expiration.
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}

/// Реальные часы через `SystemTime::now()`. При панике (часы до UNIX
/// epoch) возвращает 0 — это лишь дефолт для production, в тестах
/// используется `MockClock`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Кеширующий DNS-резолвер.
///
/// Конструируется через `new(inner, cache, default_ttl_seconds)` с
/// `SystemClock` — это типичный путь. Для тестов есть
/// `with_clock(inner, cache, default_ttl_seconds, clock)`.
pub struct CachedDnsResolver {
    inner: Arc<dyn DnsResolver>,
    cache: Arc<DnsCache>,
    default_ttl_seconds: i64,
    clock: Arc<dyn Clock>,
}

impl CachedDnsResolver {
    /// `default_ttl_seconds` — TTL для каждой записи (от `cached_at`).
    /// Рекомендуется 300 (5 мин) для browser-use, как делают Chrome /
    /// Firefox для своего DNS-cache.
    pub fn new(
        inner: Arc<dyn DnsResolver>,
        cache: Arc<DnsCache>,
        default_ttl_seconds: i64,
    ) -> Self {
        Self {
            inner,
            cache,
            default_ttl_seconds,
            clock: Arc::new(SystemClock),
        }
    }

    /// То же, что `new`, но с подменяемым clock (тесты).
    pub fn with_clock(
        inner: Arc<dyn DnsResolver>,
        cache: Arc<DnsCache>,
        default_ttl_seconds: i64,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            inner,
            cache,
            default_ttl_seconds,
            clock,
        }
    }
}

impl DnsResolver for CachedDnsResolver {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let now = self.clock.now_unix();

        // Попытка cache hit. Любая ошибка кэша (SQL, mutex) НЕ должна
        // обрушить fetch — деградируем до inner resolve. Здесь
        // map(...).ok() сбрасывает Err в None, что симметрично с «cache
        // miss».
        if let Some(entry) = self.cache.get(hostname, now).ok().flatten()
            && entry.is_fresh(now)
        {
            let parsed: Vec<SocketAddr> = entry
                .addresses
                .iter()
                .filter_map(|s| IpAddr::from_str(s.trim()).ok())
                .map(|ip| SocketAddr::new(ip, port))
                .collect();
            if !parsed.is_empty() {
                return Ok(parsed);
            }
            // Пустой/корраптный entry — игнорируем, идём в inner.
        }

        let addrs = self.inner.resolve(hostname, port)?;
        if !addrs.is_empty() {
            let strings: Vec<String> = addrs.iter().map(|a| a.ip().to_string()).collect();
            // Ошибка `put` (диск переполнен, locked DB) — не должна
            // помешать fetch: cache best-effort.
            let _ = self
                .cache
                .put(hostname, &strings, now, self.default_ttl_seconds);
        }
        Ok(addrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use lumen_core::error::Error;

    /// Test-only mock resolver: счётчик вызовов + возвращает фиксированный
    /// набор IP-строк (которые мы потом сравним).
    struct CountingResolver {
        ips: Vec<IpAddr>,
        calls: AtomicUsize,
    }

    impl CountingResolver {
        fn new(ips: Vec<IpAddr>) -> Self {
            Self {
                ips,
                calls: AtomicUsize::new(0),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl DnsResolver for CountingResolver {
        fn resolve(&self, _hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .ips
                .iter()
                .map(|ip| SocketAddr::new(*ip, port))
                .collect())
        }
    }

    /// Resolver, который всегда отдаёт `Err`. Полезен для теста, что
    /// `Err` пробрасывается, и что cache miss не зависает.
    struct AlwaysFailingResolver;

    impl DnsResolver for AlwaysFailingResolver {
        fn resolve(&self, hostname: &str, _port: u16) -> Result<Vec<SocketAddr>> {
            Err(Error::Network(format!("synthetic NXDOMAIN: {hostname}")))
        }
    }

    /// Программируемые часы. `set(t)` устанавливает текущее время;
    /// каждый `now_unix()` возвращает последнее установленное.
    struct MockClock(AtomicI64);

    impl MockClock {
        fn at(t: i64) -> Self {
            Self(AtomicI64::new(t))
        }
        fn set(&self, t: i64) {
            self.0.store(t, Ordering::SeqCst);
        }
    }

    impl Clock for MockClock {
        fn now_unix(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn ipv4(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    #[test]
    fn miss_then_hit_avoids_second_inner_call() {
        let inner = Arc::new(CountingResolver::new(vec![ipv4("93.184.216.34")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock);

        let a = r.resolve("example.com", 80).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].to_string(), "93.184.216.34:80");
        assert_eq!(inner.call_count(), 1);

        // Через 100 секунд — кэш ещё свежий (TTL 300), inner не дёргается.
        let b = r.resolve("example.com", 80).unwrap();
        assert_eq!(b, a);
        assert_eq!(inner.call_count(), 1, "cache hit, inner не зван");
    }

    #[test]
    fn expired_entry_triggers_re_resolve() {
        let inner = Arc::new(CountingResolver::new(vec![ipv4("93.184.216.34")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock.clone());

        r.resolve("example.com", 80).unwrap();
        assert_eq!(inner.call_count(), 1);

        // Перескакиваем за TTL — следующий resolve опять идёт в inner.
        clock.set(1000 + 301);
        r.resolve("example.com", 80).unwrap();
        assert_eq!(inner.call_count(), 2);
    }

    #[test]
    fn port_substituted_per_call_not_cached() {
        // Кэш хранит только IP-адреса, port подставляется при build-е
        // SocketAddr на каждый resolve. Поэтому resolve("h", 80) и
        // resolve("h", 443) на cache hit дают разные SocketAddr-ы с
        // одним IP — а inner зовётся один раз (для первого resolve).
        let inner = Arc::new(CountingResolver::new(vec![ipv4("10.0.0.1")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock);

        let a80 = r.resolve("h.test", 80).unwrap();
        let a443 = r.resolve("h.test", 443).unwrap();
        assert_eq!(a80[0].to_string(), "10.0.0.1:80");
        assert_eq!(a443[0].to_string(), "10.0.0.1:443");
        assert_eq!(inner.call_count(), 1, "второй вызов — cache hit на тот же host");
    }

    #[test]
    fn ipv6_addresses_round_trip_through_cache() {
        let ip = IpAddr::from_str("2001:db8::1").unwrap();
        let inner = Arc::new(CountingResolver::new(vec![ip]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock);

        let a = r.resolve("v6.test", 8080).unwrap();
        assert_eq!(a[0].to_string(), "[2001:db8::1]:8080");

        // Снова — из кэша, тот же результат, inner НЕ вызывался во второй раз.
        let b = r.resolve("v6.test", 8080).unwrap();
        assert_eq!(b, a);
        assert_eq!(inner.call_count(), 1);
    }

    #[test]
    fn multiple_addresses_all_cached_and_returned() {
        let inner = Arc::new(CountingResolver::new(vec![
            ipv4("1.1.1.1"),
            ipv4("1.0.0.1"),
        ]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock);

        let a = r.resolve("one.one.one.one", 53).unwrap();
        assert_eq!(a.len(), 2);
        let b = r.resolve("one.one.one.one", 53).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b, a);
        assert_eq!(inner.call_count(), 1);
    }

    #[test]
    fn inner_err_propagates_and_skips_cache_put() {
        let inner = Arc::new(AlwaysFailingResolver);
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner, cache.clone(), 300, clock);

        let err = r.resolve("nx.test", 80).expect_err("must Err");
        assert!(format!("{err:?}").contains("NXDOMAIN"));
        // Кэш пустой — отрицательное кэширование не делаем.
        assert!(cache.get("nx.test", 1000).unwrap().is_none());
    }

    #[test]
    fn corrupt_cache_entry_falls_back_to_inner() {
        // Записываем в кэш руками битый IP-адрес. На resolve CachedDnsResolver
        // должен заметить, что parsed list пустой, и обратиться к inner.
        let inner = Arc::new(CountingResolver::new(vec![ipv4("9.9.9.9")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        cache
            .put(
                "corrupt.test",
                &["not-an-ip-address".to_owned(), "::bad-stuff".to_owned()],
                1000,
                300,
            )
            .unwrap();

        let clock = Arc::new(MockClock::at(1100));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache.clone(), 300, clock);

        let addrs = r.resolve("corrupt.test", 80).unwrap();
        assert_eq!(addrs[0].to_string(), "9.9.9.9:80");
        assert_eq!(inner.call_count(), 1, "fallback на inner состоялся");

        // Cache теперь перезаписан валидными адресами от inner — следующий
        // resolve должен быть cache-hit.
        let again = r.resolve("corrupt.test", 80).unwrap();
        assert_eq!(again, addrs);
        assert_eq!(inner.call_count(), 1);
    }

    #[test]
    fn different_hostnames_independent_caches() {
        let inner = Arc::new(CountingResolver::new(vec![ipv4("8.8.8.8")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner.clone(), cache, 300, clock);

        r.resolve("a.example", 80).unwrap();
        r.resolve("b.example", 80).unwrap();
        r.resolve("a.example", 80).unwrap();
        r.resolve("b.example", 80).unwrap();
        assert_eq!(inner.call_count(), 2, "по разу на host, потом два cache-hit");
    }

    #[test]
    fn implements_dns_resolver_object_safe() {
        // Object-safety: можно положить в Arc<dyn DnsResolver>, который
        // ждёт HttpClient.with_dns_resolver(...).
        let inner = Arc::new(CountingResolver::new(vec![ipv4("127.0.0.1")]));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let r: Arc<dyn DnsResolver> = Arc::new(CachedDnsResolver::new(inner, cache, 60));
        let _ = r.resolve("localhost", 80);
    }

    /// Resolver, который держит lock на Mutex<bool> и при вызове падает
    /// с panic если уже вызывался — служит assertion-ом, что inner НЕ
    /// дёргается при cache-hit.
    struct PanicOnSecondCall(Mutex<bool>);

    impl DnsResolver for PanicOnSecondCall {
        fn resolve(&self, _hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
            let mut called = self.0.lock().unwrap();
            assert!(!*called, "resolver called twice — cache не сработал");
            *called = true;
            Ok(vec![SocketAddr::new(ipv4("1.2.3.4"), port)])
        }
    }

    #[test]
    fn second_call_strictly_skips_inner_on_fresh_hit() {
        let inner = Arc::new(PanicOnSecondCall(Mutex::new(false)));
        let cache = Arc::new(DnsCache::open_in_memory().unwrap());
        let clock = Arc::new(MockClock::at(1000));
        let r = CachedDnsResolver::with_clock(inner, cache, 300, clock);

        r.resolve("once.test", 80).unwrap();
        // Если cache miss — inner panic-нет.
        r.resolve("once.test", 80).unwrap();
        r.resolve("once.test", 80).unwrap();
    }

    #[test]
    fn system_clock_is_monotonic_within_reason() {
        let c = SystemClock;
        let a = c.now_unix();
        let b = c.now_unix();
        assert!(b >= a);
        assert!(a > 1_700_000_000, "unix time после 2023");
    }
}
