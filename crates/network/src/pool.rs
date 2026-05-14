//! Пул idle keep-alive соединений.
//!
//! По ключу `(host, port, is_tls)` хранит небольшой LIFO-стек уже открытых
//! `Connection`-ов, готовых к отправке следующего запроса. Pool потокобезопасен
//! (`Send + Sync`) — `HttpClient` может делиться им между fetch-вызовами и,
//! опционально, между несколькими экземплярами клиента через `Arc`.
//!
//! ## Что мы НЕ делаем (намеренно)
//!
//! - **Бесконечно не храним.** Соединение в пуле, переждавшее `IDLE_TIMEOUT`,
//!   при следующей попытке `acquire` выбрасывается без использования: сервер
//!   почти наверняка уже закрыл такой keep-alive. Подбор значения — короче,
//!   чем дефолтные 60–75 секунд большинства серверов, с запасом на сетевую
//!   задержку. Если сервер закроет раньше — словим stale-error и сделаем
//!   retry на свежем connect-е.
//! - **Не ограничиваем общее число idle-соединений в пуле.** Phase 0 живёт в
//!   одном процессе с однопоточным fetch-ом и редко имеет больше горстки
//!   разных origin-ов; общий cap не нужен. Per-host cap `MAX_IDLE_PER_HOST`
//!   защищает от деградации пула, если кто-то начнёт сыпать запросы в один
//!   и тот же сервер быстрее, чем пул успевает их использовать.
//! - **Не делаем active health-check** (peek `TcpStream` non-blocking + read 0
//!   байт перед использованием). Полагаемся на retry-on-stale в `lib.rs`:
//!   первый write/read на закрытом соединении упадёт, мы откроем новое.
//!   Активный peek с TLS-потоком сложнее (надо различать TLS-уровень и
//!   raw-уровень EOF), без него можно обойтись в Phase 0.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::Connection;

/// Сколько idle-соединений хранить на один origin. Современные браузеры
/// держат 6 параллельных коннектов на host — для последовательного клиента
/// этого с запасом.
const MAX_IDLE_PER_HOST: usize = 6;

/// Соединение, не использовавшееся дольше этого, считается stale и
/// выбрасывается. Короче серверного keep-alive timeout (Apache default 5 с,
/// nginx 75 с) — берём осторожно.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Ключ пула: один origin = один (host, port, is_tls). Host хранится в
/// ASCII-форме (Punycode) — RFC 7230 §5.4.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub(crate) struct PoolKey {
    pub host: String,
    pub port: u16,
    pub is_tls: bool,
}

struct Entry {
    conn: Connection,
    returned_at: Instant,
}

/// Потокобезопасный пул keep-alive соединений. По умолчанию пуст; заполняется
/// по мере того, как `HttpClient` завершает запросы и возвращает живые
/// соединения через `release`.
pub struct ConnectionPool {
    by_origin: Mutex<HashMap<PoolKey, Vec<Entry>>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            by_origin: Mutex::new(HashMap::new()),
        }
    }

    /// Забрать idle-соединение, если для данного origin-а есть свежее
    /// (не превысившее `IDLE_TIMEOUT`). Stale-соединения, найденные на верху
    /// стека, выбрасываются без try-use.
    pub(crate) fn acquire(&self, key: &PoolKey) -> Option<Connection> {
        let mut guard = self.by_origin.lock().ok()?;
        let bucket = guard.get_mut(key)?;
        while let Some(entry) = bucket.pop() {
            if entry.returned_at.elapsed() <= IDLE_TIMEOUT {
                return Some(entry.conn);
            }
            // Stale — пусть drop закрывает сокет. Продолжаем доставать.
        }
        None
    }

    /// Вернуть живое (не `closed`) соединение в пул. Если для origin-а уже
    /// набралось `MAX_IDLE_PER_HOST` соединений — самое старое выбрасывается
    /// (drop закрывает сокет), новое кладётся сверху.
    pub(crate) fn release(&self, key: PoolKey, conn: Connection) {
        let Ok(mut guard) = self.by_origin.lock() else {
            return;
        };
        let bucket = guard.entry(key).or_default();
        if bucket.len() >= MAX_IDLE_PER_HOST {
            // Удаляем самый старый (дно стека) — у него меньше шансов выжить
            // на следующем acquire. swap_remove(0) переставит последний на
            // место первого, но порядок для LIFO нам важен только в смысле
            // «новое — наверх», поэтому просто `remove(0)`.
            bucket.remove(0);
        }
        bucket.push(Entry {
            conn,
            returned_at: Instant::now(),
        });
    }

    /// Сколько idle-соединений сейчас в пуле для данного origin-а. Удобно
    /// для тестов и метрик; на hot-path не используется.
    pub fn idle_count(&self, host: &str, port: u16, is_tls: bool) -> usize {
        let key = PoolKey {
            host: host.to_owned(),
            port,
            is_tls,
        };
        self.by_origin
            .lock()
            .map(|g| g.get(&key).map(Vec::len).unwrap_or(0))
            .unwrap_or(0)
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let by_origin = self.by_origin.lock();
        match by_origin {
            Ok(guard) => {
                let total: usize = guard.values().map(Vec::len).sum();
                f.debug_struct("ConnectionPool")
                    .field("origins", &guard.len())
                    .field("idle_total", &total)
                    .finish()
            }
            Err(_) => f.debug_struct("ConnectionPool").field("poisoned", &true).finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pool_returns_none() {
        let pool = ConnectionPool::new();
        let key = PoolKey {
            host: "example.com".to_owned(),
            port: 80,
            is_tls: false,
        };
        assert!(pool.acquire(&key).is_none());
        assert_eq!(pool.idle_count("example.com", 80, false), 0);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let pool = ConnectionPool::new();
        let s = format!("{pool:?}");
        assert!(s.contains("ConnectionPool"));
    }

    // Семантика acquire/release с реальными `Connection` проверяется
    // E2E-тестами в `lib.rs::tests` (two_fetches_reuse_one_tcp_connection,
    // server_says_connection_close_drops_pool_entry,
    // stale_pooled_connection_triggers_retry).
}
