//! Memory pressure poll loop — throttled platform memory monitor.
//!
//! The shell calls [`MemoryPollTick::tick`] each `about_to_wait`.  Every
//! [`POLL_INTERVAL`] seconds the underlying [`MemoryPressureSource`] is queried;
//! when the level reaches [`MemoryPressureLevel::Medium`] or above,
//! [`CacheRegistry::broadcast_pressure`] is called so every registered cache
//! can evict proportionally.

use std::time::{Duration, Instant};

use lumen_core::ext::{CacheRegistry, MemoryPressureLevel, MemoryPressureSource};

/// How often to query the OS for memory pressure.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Throttled memory pressure poller.
///
/// Wraps a platform [`MemoryPressureSource`] and polls it at most once per
/// [`POLL_INTERVAL`].  On [`MemoryPressureLevel::Medium`] or [`MemoryPressureLevel::High`],
/// [`tick`](Self::tick) calls [`CacheRegistry::broadcast_pressure`] on all registered
/// caches so each evicts proportionally without the caller needing to know about
/// individual cache implementations.
pub struct MemoryPollTick {
    /// OS-level pressure source.
    source: Box<dyn MemoryPressureSource>,
    /// Instant of the last poll (or creation time).
    last_poll: Instant,
    /// Last sampled pressure level — exposed for diagnostics.
    last_level: MemoryPressureLevel,
}

impl MemoryPollTick {
    /// Create a new poller using the given platform source.
    ///
    /// The first poll fires after [`POLL_INTERVAL`] elapses, not immediately.
    pub fn new(source: Box<dyn MemoryPressureSource>) -> Self {
        Self {
            source,
            last_poll: Instant::now(),
            last_level: MemoryPressureLevel::Low,
        }
    }

    /// Poll memory pressure and broadcast to `registry` if pressure is Medium or High.
    ///
    /// Throttled to once per [`POLL_INTERVAL`].  Returns `Some(level)` when a
    /// broadcast was triggered; `None` when the interval has not elapsed or
    /// pressure is `Low`.
    pub fn tick(&mut self, registry: &mut CacheRegistry) -> Option<MemoryPressureLevel> {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return None;
        }
        self.last_poll = Instant::now();
        let level = self.source.poll_current();
        self.last_level = level;
        if level >= MemoryPressureLevel::Medium {
            registry.broadcast_pressure(level);
            Some(level)
        } else {
            None
        }
    }

    /// Last sampled pressure level.  May be stale by up to [`POLL_INTERVAL`].
    #[allow(dead_code)]
    pub fn last_level(&self) -> MemoryPressureLevel {
        self.last_level
    }
}

/// Build the appropriate [`MemoryPressureSource`] for the current platform.
///
/// Returns [`lumen_core::ext::NullMemoryPressureSource`] on platforms without
/// a dedicated implementation.
pub fn platform_source() -> Box<dyn MemoryPressureSource> {
    cfg_if_platform_source()
}

#[cfg(target_os = "windows")]
fn cfg_if_platform_source() -> Box<dyn MemoryPressureSource> {
    Box::new(lumen_core::memory_pressure::Win32MemoryPressureSource)
}

#[cfg(target_os = "linux")]
fn cfg_if_platform_source() -> Box<dyn MemoryPressureSource> {
    Box::new(lumen_core::memory_pressure::LinuxMemoryPressureSource)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn cfg_if_platform_source() -> Box<dyn MemoryPressureSource> {
    Box::new(lumen_core::ext::NullMemoryPressureSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::NullMemoryPressureSource;

    fn expired_tick() -> MemoryPollTick {
        let mut t = MemoryPollTick::new(Box::new(NullMemoryPressureSource));
        t.last_poll = Instant::now() - POLL_INTERVAL - Duration::from_secs(1);
        t
    }

    #[test]
    fn throttled_immediately_after_creation() {
        let mut tick = MemoryPollTick::new(Box::new(NullMemoryPressureSource));
        let mut reg = CacheRegistry::new();
        assert!(tick.tick(&mut reg).is_none(), "must be throttled right after creation");
    }

    #[test]
    fn null_source_returns_none_after_interval() {
        let mut tick = expired_tick();
        let mut reg = CacheRegistry::new();
        // NullMemoryPressureSource always returns Low — no broadcast expected.
        assert!(tick.tick(&mut reg).is_none(), "Low pressure must not trigger broadcast");
    }

    #[test]
    fn last_level_starts_low() {
        let tick = MemoryPollTick::new(Box::new(NullMemoryPressureSource));
        assert_eq!(tick.last_level(), MemoryPressureLevel::Low);
    }

    #[test]
    fn timer_resets_after_poll() {
        let mut tick = expired_tick();
        let mut reg = CacheRegistry::new();
        // First expired poll — resets timer.
        let _ = tick.tick(&mut reg);
        // Immediate second poll — throttled.
        assert!(tick.tick(&mut reg).is_none(), "must be throttled after firing");
    }

    struct AlwaysMediumSource;
    impl MemoryPressureSource for AlwaysMediumSource {
        fn poll_current(&self) -> MemoryPressureLevel {
            MemoryPressureLevel::Medium
        }
    }

    struct AlwaysHighSource;
    impl MemoryPressureSource for AlwaysHighSource {
        fn poll_current(&self) -> MemoryPressureLevel {
            MemoryPressureLevel::High
        }
    }

    #[test]
    fn medium_pressure_triggers_broadcast() {
        let mut tick = MemoryPollTick {
            source: Box::new(AlwaysMediumSource),
            last_poll: Instant::now() - POLL_INTERVAL - Duration::from_secs(1),
            last_level: MemoryPressureLevel::Low,
        };
        let mut reg = CacheRegistry::new();
        let result = tick.tick(&mut reg);
        assert_eq!(result, Some(MemoryPressureLevel::Medium));
        assert_eq!(tick.last_level(), MemoryPressureLevel::Medium);
    }

    #[test]
    fn high_pressure_triggers_broadcast() {
        let mut tick = MemoryPollTick {
            source: Box::new(AlwaysHighSource),
            last_poll: Instant::now() - POLL_INTERVAL - Duration::from_secs(1),
            last_level: MemoryPressureLevel::Low,
        };
        let mut reg = CacheRegistry::new();
        let result = tick.tick(&mut reg);
        assert_eq!(result, Some(MemoryPressureLevel::High));
    }

    #[test]
    fn broadcast_reaches_registered_cache() {
        use lumen_core::EvictableCache;

        struct CountingCache {
            evictions: usize,
        }
        impl EvictableCache for CountingCache {
            fn on_memory_pressure(&mut self, _level: MemoryPressureLevel) {
                self.evictions += 1;
            }
            fn used_bytes(&self) -> usize { 0 }
            fn budget_bytes(&self) -> usize { usize::MAX }
            fn clear(&mut self) {}
            fn cache_name(&self) -> &'static str { "test" }
        }

        let counting = CountingCache { evictions: 0 };
        // We can't inspect the cache after moving it into the registry, so we
        // just verify the tick returns Some — meaning broadcast was called.
        let mut tick = MemoryPollTick {
            source: Box::new(AlwaysMediumSource),
            last_poll: Instant::now() - POLL_INTERVAL - Duration::from_secs(1),
            last_level: MemoryPressureLevel::Low,
        };
        let mut reg = CacheRegistry::new();
        reg.register(Box::new(counting));
        assert!(tick.tick(&mut reg).is_some(), "broadcast must fire with registered cache");
    }

    #[test]
    fn platform_source_returns_box() {
        let _src = platform_source();
        // Just verify it compiles and returns a valid source on this platform.
        // NullMemoryPressureSource always returns Low.
    }
}
