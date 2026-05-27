# ADR-007: Anti-detection as a privacy stack, not a circumvention tool

## Status

Accepted

## Date

2026-05-27

## Context

A user has the right to read a publicly accessible website from their own device. This is the same principle that Firefox, Brave, and Tor operate under: the user agent serves the user, not the website operator. Anti-bot systems (Cloudflare, DataDome, Akamai, PerimeterX, Kasada, Imperva) are deployed by site operators to filter automated traffic, but their detection methods routinely false-positive on legitimate non-Chrome browsers, on privacy-hardened users, and on users with non-typical hardware (Linux desktops, ARM machines, older devices).

The detection methods in 2026 work at multiple layers:

1. **Browser surface API leaks** — `navigator.webdriver`, CDP side-channels in the JS environment, `chrome.runtime` object presence, headless-specific WebDriver hooks.
2. **TLS fingerprint** — JA3/JA4 hash derived from cipher suite ordering, extension list, supported groups. `rustls` produces a different JA3 from Chrome.
3. **HTTP layer** — header order, header casing, HTTP/2 SETTINGS frame values, HTTP/2 stream priority patterns, `Accept-Encoding` ordering.
4. **Rendering fingerprint** — canvas pixel hash, WebGL renderer string + shader compilation timing, AudioContext fingerprint, font enumeration, codec support matrix.
5. **Behavioral telemetry** — mouse trajectory shape (real users move along imperfect Bézier curves; legacy bots move in straight lines and click geometric centers instantly), keystroke timing variance, scroll-event distribution.
6. **Network identity** — IP ASN reputation (datacenter IPs are flagged), TLS session resumption patterns.

Lumen has unique architectural opportunities here:

- It has **no debug-protocol bolted on** (no CDP attached to the JS runtime, no `navigator.webdriver`) — so the most basic detection markers do not exist by construction. This is a side effect of [ADR-006](ADR-006-automation-api.md), not a deliberate stealth feature.
- It controls its own TLS stack (`rustls`, ADR-002 permanent exception #3), so cipher ordering and ALPN/extension ordering are a configuration choice, not a Chromium-defined constant.
- It controls its own HTTP/1 and HTTP/2 implementations (`lumen-network`), so header ordering is a deliberate choice, not an accident of `reqwest`/`hyper`.
- It controls its own rendering pipeline (`lumen-paint`, `lumen-font`, `lumen-canvas`), so canvas/WebGL/audio fingerprint determinism is feasible (§9.5 already has Brave-style randomization).

Legally: the right of a user to access publicly available web content from a personal browser is well-established. The U.S. Supreme Court (*Van Buren v. United States*, 2021) narrowed CFAA's "exceeds authorized access" so that public-page access is not a federal computer crime. *hiQ Labs v. LinkedIn* (9th Cir., 2022) reinforced this. EU GDPR and DSA do not prohibit private browsing. There is no Russian statute against browsing public information from a personal client.

However, three categories of "bypass" are clearly different and Lumen must not be confused with the latter two:

| Category | Example | Lumen position |
|---|---|---|
| **Privacy** | User reads news.example.com from their Lumen browser; Cloudflare doesn't pop a CAPTCHA because Lumen looks like a normal browser. | **Yes — default behavior.** Same as Firefox Strict mode. |
| **Disputed grey area** | User mirrors a publicly-readable Wikipedia page for offline study; user scrapes a public business directory at human speed. | **Not Lumen's concern.** Lumen is a browser, not a scraping framework. Users can build their own scrapers using `lumen-driver`. |
| **Abuse** | Bypassing CAPTCHA to mass-create fake accounts; bypassing fraud-detection on a bank; bypassing paywalls that contractually require human auth. | **Explicitly out of scope.** Lumen ships no CAPTCHA solver, no built-in IP rotation, no anti-fraud-circumvention features. |

## Decision

Treat anti-detection capabilities as a **layered privacy stack**, delivered by default to all Lumen users with no marketing, no "stealth mode" branding, and no opt-in for the basic protections. The stack mirrors what Firefox Strict / Brave / Tor already do, extended where Lumen's own-stack control allows.

### Layer 1 — Surface API: no automation markers (default, always on)

- `navigator.webdriver` is **not present** (not `false`, not present at all — same as a clean Chrome without `--enable-automation`).
- No `chrome.runtime`, no `__playwright`, no `__puppeteer`, no `__nightmare`, no `cdc_*` (ChromeDriver), no `_phantom`, no `callPhantom`, no `Buffer` global, no `emit` on `window`.
- The JS runtime (`rquickjs` Phase 0, V8 Phase 3+) is **not instrumented for automation**. Automation goes through `BrowserSession` trait (ADR-006), which never touches the JS environment unless the page itself accesses it (e.g., `eval_js()` runs as an ordinary script, leaving no trace).
- DOM `event.isTrusted = true` for native-injected input (ADR-006, task 8C.2), because the events enter through the same path as OS events.

### Layer 2 — TLS fingerprint (default + profile-switchable)

- `rustls` is configured with a **cipher suite list and order matching the current stable Chrome (default profile)**. Extension list and supported groups likewise.
- ALPN preference order: `h2`, `http/1.1` — same as Chrome.
- This is **the default**. Reason: the user did not choose `rustls`-specific cipher ordering; it is an artifact of our dependency, and a site operator should not be able to single out Lumen users just because we pick a different Rust TLS library.
- **Profile-switchable**: a privacy-strict profile can use `rustls` defaults, a corporate profile can pin a specific JA3, a Tor profile uses Tor Browser's JA3.
- This is **not TLS mimicry to impersonate**; it is `rustls` configured the way `rustls` could already configure itself. We do not patch crypto, we choose parameters.

### Layer 3 — HTTP layer (default + profile-switchable)

- HTTP/1.1 request line and header ordering mirrors current Chrome: `User-Agent`, `Accept`, `Accept-Encoding`, `Accept-Language`, etc. in Chrome's order, with Chrome's casing.
- HTTP/2 SETTINGS frame values match Chrome (`SETTINGS_HEADER_TABLE_SIZE = 65536`, `SETTINGS_MAX_CONCURRENT_STREAMS = 1000`, `SETTINGS_INITIAL_WINDOW_SIZE = 6291456`, etc.).
- HTTP/2 stream priority frames mirror Chrome's pattern (priority tree).
- `Accept-Language` defaults to `en-US,en;q=0.9` unless the user overrides (does not silently expose the user's real locale).
- Client Hints (`Sec-CH-UA`, etc.) — Lumen advertises its own UA via Client Hints when asked, or returns nothing on Strict — same as Tor.

### Layer 4 — Rendering fingerprint (default = Brave-style; existing §9.5)

Already specified in §9.5. Reaffirmed here:

- Canvas randomization with per-session deterministic seed (Brave-style).
- WebGL renderer/vendor strings normalized to a small fixed set.
- AudioContext noise injection.
- Font enumeration limited to a stable whitelist + bundled fonts only on Strict.
- `Date.getTimezoneOffset()` returnable as UTC on Strict.
- `screen.width/height` snap-to-100px on Strict.
- `navigator.hardwareConcurrency` clamped to a fixed value on Strict.
- `Battery API` — disabled (returns no information) on Strict.
- `WebRTC` — mDNS host candidates only (no public IP leak) — same as Brave/Safari.

### Layer 5 — Behavioral input (opt-in for automation API only)

- For `BrowserSession::input_event()` (ADR-006 task 8C), provide an opt-in `mode: InputMode::HumanLike` that produces Bézier-curve mouse paths, variable inter-keystroke timing (Gaussian around natural rhythm), and small dwell-times before clicks. **This is for testers**, who want their automated test runs to exercise the same code paths a real user would (event coalescing, slow-pointer logic, hover transitions) — not as a stealth feature.
- Real human input through the shell (mouse, keyboard, touch) needs no mimicry — it already is human.

### Layer 6 — Per-profile / per-context configuration

- Three default profiles, extending existing §9.5 Standard/Strict/Tor:
  - **Standard** (default for normal users) — Layers 1+2+3 + §9.5 anti-fp at low intensity.
  - **Strict** — Layers 1+2+3 + §9.5 anti-fp at high intensity + WebRTC mDNS-only + Client Hints disabled.
  - **Tor-mode** — Strict + Tor circuit + Tor Browser fingerprint pinning (JA3 + UA + screen + fonts) + no persistent state.
- **Per-context override** via `BrowserSession::set_fingerprint_profile(profile)` for automation users who deliberately want a specific identity.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Do nothing; let Lumen be detectable as "non-Chrome" everywhere | Users get false-positive CAPTCHAs on legitimate sites. Brave/Firefox/Tor have all rejected this; we should too. |
| Build a full "anti-detection toolkit" with CAPTCHA solver, IP rotation, proxy fleet, mouse-mimic-on-demand | Crosses from privacy into circumvention tooling. Attracts regulatory heat (UK Online Safety Act, EU DSA), risks app-store removal, and is misaligned with Lumen's user-agent ethics. Users who want this build their own with `lumen-driver`. |
| Make it opt-in ("Enable stealth mode") | Privacy must be the default, not a setting. The same principle as no telemetry by default (§9.7). |
| Impersonate a specific Chrome version exactly (full JA3 + UA + WebGL + canvas pinning at all times) | We are not Chrome, claiming to be Chrome in detail is dishonest to the site operator. We mimic only the **layers that should not single out the user agent** (TLS, HTTP, surface-API absences). We keep our own UA string ("Lumen/0.x") because that is honest, and a site that blocks "Lumen" is the site's prerogative — same as a site that blocks Firefox. |

## Consequences

- **Positive:**
  - Lumen users are not false-positive-flagged by Cloudflare/DataDome/Akamai/etc. on routine browsing.
  - No `navigator.webdriver`, no CDP side-channels, no automation hooks — by construction, not by patching (see ADR-006).
  - The privacy story is coherent: anti-fingerprinting at canvas level (§9.5) connects with anti-fingerprinting at network level (TLS/HTTP) in one principled stack.
  - Automation users (testers, AI agents) get a browser that does not flag itself as automated — testing Cloudflare-protected sites becomes possible without separate stealth-plugins.
  - The default UA stays `Lumen/0.x` — honest. Site operators who object to non-Chrome browsers have the same recourse they have against Firefox: ask their users to "use Chrome" (and lose those users).

- **Negative / trade-offs:**
  - Some sites that fingerprint aggressively will still flag Lumen as "unknown browser" and gate features. Acceptable trade-off; same problem Firefox/Brave have.
  - Engineering cost: TLS/HTTP layer-matching to current Chrome requires periodic update as Chrome's defaults shift (one PR per Chrome major release, low effort).
  - We do not get the SaaS-tier of bypass capabilities (residential proxies, CAPTCHA solvers). That is intentional.

- **Future / red lines (never crossed):**
  - **No CAPTCHA solver, on-device or via third-party service.** Sites have a legitimate interest in distinguishing humans from automated scripts in certain contexts. Lumen will not undermine that.
  - **No built-in IP rotation, no built-in residential-proxy integration.** Network identity is the user's choice and the user's responsibility. Lumen has no opinion about which IP the user routes through; it does not facilitate large-scale rotation.
  - **No "anti-fraud-detection" features for banking, payment, or government services.** These systems guard against real harm; Lumen does not help users circumvent them.
  - **No marketing language like "scraping browser", "anti-bot bypass", "stealth automation".** Lumen is a privacy browser. The fact that it is also a clean automation surface (ADR-006) is communicated in technical docs to developers, not in product copy.
  - **No paid "stealth subscription tier" or any commercial product around bypass.** This would invert the economics and create incentives to keep users blocked-by-default.
- **Future / graduation triggers:**
  - If a major anti-bot vendor publishes a detection method specifically for Lumen, the response is a one-PR layer-update, not a new product strategy.
  - If a regulator requests changes (e.g., a court order against bypassing a specific authentication flow), Lumen complies for that specific flow without altering the general privacy stack.

## Performance gate (binding)

Anti-detection layers (1-6, all six) **must not regress `lumen-bench` median by more than 5%** vs the recorded baseline. This applies to the **default user build** with the default profile active (no `--mcp` / `--bidi-port`, no `--strict` profile, no `--tor`). The default privacy stack is what every Lumen user gets, so it must stay free.

Specifically:

- **Layer 1 (surface API absences)** — must be net-neutral or net-positive (fewer JS globals = faster `window` access).
- **Layer 2 (TLS Chrome-matching)** — configuration of `rustls`, evaluated at handshake; no per-request cost.
- **Layer 3 (HTTP/H2 Chrome-matching)** — header-ordering structure, evaluated at request build; sub-microsecond per request.
- **Layer 4 (canvas/WebGL/audio noise)** — cost lives in **rare JS API calls** (`getImageData`, `getFrequencyData`), not in paint/decode hot path. Bench page (`samples/page.html`) must not exercise these APIs measurably.
- **Layer 5 (behavioral mimicry)** — opt-in only, zero default cost.
- **Layer 6 (profiles)** — config struct, loaded once at startup.

If a layer regresses `lumen-bench`:

1. Re-architect to lazy-evaluate (compute only on the JS API call, not on every page).
2. Reduce intensity at Standard profile, keep the more expensive variant for Strict.
3. Document the regression with architectural justification and get reviewer sign-off.

CI gate (task 9G.3 in Roadmap) runs `cargo run -p lumen-bench --release` per PR touching `lumen-network` / `lumen-canvas` / `lumen-js` / `lumen-storage::profiles` and fails on >5% median regression of **either** time-axis or RAM-axis (RAM-axis added by ADR-008 task 9G.5) vs `bench/baseline.json`.

This gate is a hard contract: anti-detection is invisible to the user **only if** it is also invisible to the performance budget — both CPU time and RAM footprint.
