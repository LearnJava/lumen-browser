# Per-Context Isolation in BrowserSession (Task 8E, Phase 1)

## Overview

Each `BrowserSession` must be an isolated context by default (ADR-006 §6). Current state: viewport isolation is implemented; cookies, storage, cache, and fingerprint isolation are **not yet wired**.

## Isolation Scope

| Component | Current | Phase 1 Target |
|---|---|---|
| **Viewport** | ✅ Per-session | Per-session |
| **Cookies** | 🟡 Global storage, no per-session isolation | Per-session CookieJar |
| **Storage** (localStorage, sessionStorage) | 🟡 Global storage, no per-session namespace | Per-session origin-keyed store |
| **HTTP Cache** | ⬜ Not implemented | Per-session in-memory HttpCache |
| **Image Decode Cache** | 🟡 Shared global (future task 10E) | Per-session (Phase 2) |
| **Glyph Cache** | 🟡 Shared global (future task 10G) | Per-session (Phase 2) |
| **User-Agent** | ⬜ Not per-session config | Per-session fingerprint profile |
| **Fingerprint Profile** | ⬜ Not implemented | Per-session (Standard/Strict/Tor) |

## Architecture: SessionContext

To isolate cookies/storage/cache per-session, introduce an internal `SessionContext` struct:

```rust
/// Isolated context for a single BrowserSession.
pub struct SessionContext {
    /// Per-session cookie jar (origin-keyed).
    cookies: CookieJar,
    
    /// Per-session storage backends (origin-keyed).
    /// - localStorage: InMemoryStorage (cleared on navigation)
    /// - sessionStorage: InMemoryStorage (cleared on session end)
    storage: Box<dyn StorageBackend>,
    
    /// Per-session HTTP cache (in-memory for Phase 1).
    http_cache: HttpCache,
    
    /// Per-session fingerprint profile (Standard/Strict/Tor).
    fingerprint_profile: FingerprintProfile,
    
    /// Per-session user-agent string override.
    user_agent: Option<String>,
}
```

### Why SessionContext (not InProcessSession fields)?

1. Encapsulation: separates session state (DOM, layout) from resource isolation (cookies, cache).
2. Future transport-agnostic: WinitSession, MCP/BiDi adapters will reuse the same `SessionContext`.
3. Simplifies testing: mock `SessionContext` for unit tests of isolation boundaries.

## Phase 1 Deliverables

### 1. Define SessionContext

**File:** `crates/driver/src/context.rs` (new)

- `SessionContext` struct with fields above.
- Methods: `new()`, `with_fingerprint_profile()`, `with_user_agent()`.
- Integration into `InProcessSession` as private field.

### 2. Extend BrowserSession trait

**File:** `crates/driver/src/lib.rs`

Add methods (if not already present):

```rust
pub trait BrowserSession {
    /// Get current fingerprint profile (Standard/Strict/Tor).
    fn fingerprint_profile(&self) -> FingerprintProfile;
    
    /// Set fingerprint profile for future operations.
    fn set_fingerprint_profile(&mut self, profile: FingerprintProfile) -> Result<()>;
    
    /// Get current user-agent string.
    fn user_agent(&self) -> &str;
    
    /// Override user-agent for future requests.
    fn set_user_agent(&mut self, ua: &str) -> Result<()>;
}
```

### 3. Cookie Isolation

**File:** `crates/driver/src/session.rs`

- Replace global cookie jar with per-session `CookieJar` from `lumen-storage`.
- Ensure `navigate()` isolates cookies by origin (via `lumen-storage::CookieJar::get_for_url()`).
- Add test: two sessions with same URL receive different cookies.

### 4. Storage Isolation (localStorage/sessionStorage)

**File:** `crates/driver/src/session.rs`

- Create per-session `InMemoryStorage` for localStorage.
- localStorage persists within a session, cleared on navigate.
- sessionStorage cleared on session end.
- Tests: origin-keyed isolation (origin-A cannot read origin-B's storage).

### 5. HTTP Cache Isolation

**File:** `crates/driver/src/context.rs`

- Per-session `HttpCache` instance (in-memory).
- No sharing between sessions.
- Cache is keyed by request (method, URL, headers).
- Cleared on session end.

### 6. Fingerprint Profile & User-Agent

**File:** `crates/driver/src/context.rs`

- Define `FingerprintProfile` enum: `Standard`, `Strict`, `Tor`.
- Per-session profile (default: Standard).
- Profile affects:
  - User-Agent string (if not explicitly overridden).
  - TLS cipher suite ordering (TLS layer, external to this task).
  - HTTP header ordering (HTTP layer, external to this task).
  - JS API returns (canvas noise, WebGL strings, etc. — Phase 2 task).

### 7. Tests

**File:** `crates/driver/tests/isolation.rs` (new)

- `test_cookies_isolated`: two sessions, same URL, different cookies.
- `test_storage_isolated`: origin-keyed isolation for localStorage.
- `test_http_cache_isolated`: cache miss in session A after clear in session B.
- `test_fingerprint_profile_override`: set_fingerprint_profile() changes profile.
- `test_user_agent_override`: set_user_agent() changes UA string.

## Phase 1 Restrictions

- **Not included**: Image/glyph cache isolation (Phase 2 task 10E, 10G).
- **Not included**: JS APIs that expose fingerprint (Phase 2 task 9D).
- **Not included**: Deterministic mode (clock freeze, RNG seed — task 8F).

## Integration Points

- **lumen-network**: HTTP client must accept per-session HttpCache.
- **lumen-storage**: CookieJar and StorageBackend must be per-session.
- **lumen-shell**: When WinitSession is wired (task 8A.7), reuse SessionContext.

## Future Work (Phase 2+)

- Image decode cache per-session (task 10E).
- Glyph atlas eviction per-session (task 10G).
- JS fingerprint APIs (canvas, WebGL, audio) per-profile (task 9D).
- Deterministic mode (task 8F) — clock, RNG, fingerprint freeze.

## References

- ADR-006 §6: Per-context isolation by default.
- ADR-007 §6: Per-profile fingerprint configuration.
- Task 8E in STATUS-P3.md.
