//! DNS-резолверы: системный (default) и интеграционная точка для подмены.
//!
//! `lumen_core::ext::DnsResolver` — публичный trait; `SystemDnsResolver`
//! ниже — единственная реализация, живущая в lumen-network (через
//! `(host, port).to_socket_addrs()` из std). Кешированные / DoH / DoT
//! реализации — отдельные crate-ы (`lumen-storage::CachedDnsResolver`),
//! lumen-network знает их только через trait.

use std::net::{SocketAddr, ToSocketAddrs};

use lumen_core::error::{Error, Result};
use lumen_core::ext::DnsResolver;

/// DNS-резолвер на основе системного getaddrinfo (через std::net).
///
/// Default-резолвер для `HttpClient` — поведение совпадает с прежним
/// `TcpStream::connect("host:port")`, который внутренне делает то же
/// самое. Пустой результат от `to_socket_addrs` маппится в Err — не в
/// пустой Vec, потому что для системного резолвера «нет адресов» это
/// аномалия (NXDOMAIN обычно возвращается как io::ErrorKind::NotFound).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let target = (hostname, port);
        let addrs: Vec<SocketAddr> = target
            .to_socket_addrs()
            .map_err(|e| Error::Network(format!("resolve {hostname}: {e}")))?
            .collect();
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
    fn system_resolver_is_send_sync_object_safe() {
        fn check<T: Send + Sync>() {}
        check::<SystemDnsResolver>();
        // Object-safety: можно положить в Box<dyn DnsResolver>.
        let _: Box<dyn DnsResolver> = Box::new(SystemDnsResolver);
    }
}
