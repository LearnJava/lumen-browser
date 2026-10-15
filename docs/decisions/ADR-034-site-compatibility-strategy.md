# ADR-034: Site compatibility («level 0 — the site works») — one identity, OS trust, a visible compat pack, honest hand-off

## Status

Accepted (user, 2026-10-12). **Amends [ADR-007](ADR-007-anti-detection-stack.md)**: the default UA
string, Client Hints and the `Accept-Language` / time-zone defaults of the Standard profile. ADR-007's
red lines (no CAPTCHA solving, no IP rotation, no anti-fraud circumvention) are unchanged and binding
here too.

## Date

2026-10-12

## Context

The user-facing hierarchy derived in [`docs/research/browser-ux/report.md`](../research/browser-ux/report.md)
§3 puts «the site works (like in Chrome)» at level 0: a defect there devalues every other quality of
the browser. The follow-up study [`docs/research/site-compat/report.md`](../research/site-compat/report.md)
(8 areas, 238 findings checked against sources) established, for a browser on its own engine:

- **Sites break on gatekeepers before they break on layout.** 52 % of Firefox's site interventions
  are identity cut-offs (UA sniffing, «unsupported browser» banners); the largest root-cause nodes in
  Mozilla's knowledge base are «Unsupported Browser» (341 reports) and «Unsupported browser warning»
  (234), while no CSS/API node exceeds 47. Next come anti-bot systems (they flag *inconsistency*
  between TLS, HTTP/2, UA, Client Hints and the JS surface, and flag *rarity* itself), TLS trust, and
  DRM. In Russia since 2026-08-03 the major banks serve certificates chained to the Russian Trusted
  Root CA (НУЦ Минцифры) — Chrome no longer opens them; the reference browser for RU users is Yandex
  Browser (≈29 % share).
- **Users blame the unpopular browser** after one A/B in a second browser, tolerate fewer than four
  failures, and almost never report (57–67 % never do; ≈1.3 % of webcompat reports end in a fix).
  They fight for login (80 %) and streaming (83 %), not for a missing image (22 %).
- **WPT pass rate does not measure readiness** (Ladybird: 93.8 % of subtests vs Safari's 94.4 %,
  still pre-alpha).
- Neither Presto (300 M users) nor EdgeHTML (Windows default) made sites test for an alternative
  engine; every surviving engine keeps a permanent, data-driven compatibility layer (Firefox: 405
  intervention files, WebKit: ~180 quirk entries, Edge: server-side Domain Actions).

Lumen's state on `6139c57cf` (verified in code 2026-10-12): the HTTP `User-Agent` is Chrome/153 while
`navigator.userAgent` is `Lumen/0.5.0` and `navigator.userAgentData` says Chrome 114; `Sec-CH-UA` is
never sent (`client_hints_headers` has no caller outside its module); TLS trusts only the compiled-in
`webpki_roots` plus an env var — no Windows root store, so HTTPS-inspecting antivirus breaks every
site and НУЦ-chained banks fail; `IdentityCredential` is exposed but always rejects; WebRTC is a stub.
ADR-007 promised an honest `Lumen/0.x` UA; the code drifted into an inconsistent half-mask, which is
the worst of the options.

## Decision

1. **One identity, consistent in every layer.** A single `IdentityProfile` feeds HTTP `User-Agent`,
   `Sec-CH-UA*` (with GREASE per Chromium's algorithm, low-entropy hints sent by default like Chrome),
   `navigator.userAgent`/`appVersion`/`vendor`/`platform`/`productSub` in window, workers and iframes,
   `navigator.userAgentData` (incl. `getHighEntropyValues`), the shape of `window.chrome`, and the
   TLS/H2 preset. A CI gate fails on any cross-layer mismatch (metric ICR = 0).
   - **Default now:** Chrome grammar *without* a Lumen token, consistently in all layers (removes the
     contradiction without adding new risk; BUG-1113 showed the UA string, not TLS, decided refusals).
   - **Then measure** (A/B 2×2: network stack × token on the compat corpus and the anti-bot bench).
     Switch the default to Chrome grammar + trailing `Lumen/x.y` (Edge/Ladybird model) if the token
     adds refusals on ≤ 0.5 % of the corpus and on no must-work site; such sites get a per-site
     `chrome_unbranded` record. Rationale for the token as the target: Google's sign-in criterion
     («the browser identifies itself») and the «transparent» positioning.
   - Client Hints brands always follow the UA string (no half-state). «Google Chrome» as a brand only
     as a per-site compat-pack record.
   - The Chrome major version comes from signed config delivered with the compat pack; fallback is a
     4-week-cycle calculation capped at the known stable; CI keeps it within stable − 2.
   - TLS/HTTP-2 fingerprint work (replacing or reshaping `rustls`) is **not** started until the JS
     layer is consistent and the A/B attributes refusals to the stack on ≥ 3 % of the top-1000 or on
     any must-work site.
   - **A/B result (2026-10-15, 533 sites, 100 must-work; [matrix](../perf/runs/2026-10-15-ab-matrix.md)):**
     the `Lumen/x.y` token (and the `Lumen` brand in `Sec-CH-UA`) blocks 1 site = 0.19 % (fiverr-com,
     PerimeterX), no must-work site, none in the real-Chrome arm → the Р1/Р2 threshold (≤ 0.5 %) is met:
     the default may move to Chrome grammar + trailing `Lumen/x.y`, fiverr-com gets a `chrome_unbranded`
     record. Stack: Chrome lets in / Lumen blocks 25 sites (4.7 %), but only one (eapteka-ru) is
     confirmed to depend on the TLS fingerprint; 7 are the missing Russian root CA (OS trust, not
     `rustls` shape), 2 DNS, 7 JS challenges the no-JS Lumen arm cannot pass, 4 anti-bot refusals
     identical even for `curl_cffi impersonate=chrome` (JS/cookie challenge) → Р4 threshold not met,
     **the TLS/H2 project stays closed**. Р16 (device APIs, `window.chrome`) is **not measured**: the Lumen
     arm executes no JS (live window, BUG-1629); re-measure the stack share and Р16 with a JS arm.
2. **Feature detection never lies.** An API that cannot work is not exposed: no `IdentityCredential`
   until FedCM works; device APIs (USB/Serial/HID/Bluetooth) removed unless the anti-bot bench shows
   their presence is required; `RTCPeerConnection` without a stack fails honestly and the UI says
   «calls do not work yet» before the call; MSE/EME/WebCodecs are not declared without a working
   implementation; `canPlayType`/`isTypeSupported`/`decodingInfo` answer from the real decoder set.
3. **Trust.** On Windows, roots from the OS store (ROOT, CA, enterprise/GPO) are trusted in addition
   to `webpki_roots`, with a visible indicator when a chain ends in a locally installed root. Error
   pages distinguish HTTPS inspection by antivirus (naming the product), wrong system clock, captive
   portal and missing intermediates (AIA fetching). **НУЦ Минцифры**: off by default, offered at the
   first encounter, trusted only with SCTs from ≥ 2 distinct log operators from a list shipped in the
   build; a НУЦ root found in the OS store is recognized by SPKI and also requires CT; Lumen never
   writes a root into the OS. The «Proceed anyway» button is never shown on bank/HSTS hosts.
4. **Compat pack — data, not code, and always visible.** Site interventions are signed declarative
   records «root cause × site» from a closed action vocabulary, in order of preference: `css`,
   `hide_banner` (text-matched), `engine_flag` (named Chrome-ism), `protection` (exception/shim),
   `identity` (`chrome_lumen | chrome_unbranded | chrome_brand_google | firefox_win`),
   `js_transform:<id>` (built-in transforms only, ≤ 20 active), `known_issue` / `handoff_hint`.
   Matching is never broader than eTLD+1. Every record links a BUG/KB id, has `review_by`, and paired
   tests or an explicit exemption. A copy ships in the binary; remote delivery outside releases
   (stage 2) goes through a mirror reachable from Russia. `about:compat` (internal pages use the `about:` scheme; the research calls it `lumen://compat`) lists every active record
   with a per-record off switch; the page shows an indicator and a `console.info` line when a record
   applies. Bank and payment hosts accept only `identity`, `css`, `known_issue`. No remotely
   delivered executable code, no monolithic `browser.js`, no site quirks hard-coded in the engine.
5. **No dead ends; hand-off instead of a second engine.** Every recognized breakage offers an exit in
   ≤ 2 clicks, including «Open in another browser» (installed browsers from the Windows registry,
   chosen by the breakage reason; per-site rule for 30 days, re-checked after Lumen updates; never
   opened automatically). **No embedded second engine** (WebView2/CEF): it costs L–XL, brings
   Microsoft telemetry and >250 MB, bypasses the content blocker and the compat pack, and removes the
   incentive to fix the own engine (KHTML/Konqueror). Revisit only for a corporate track.
6. **Verdicts with evidence.** The browser runs the control experiment itself (reload without cache,
   «as Chrome», this site's data reset, protections off for this tab, network check) and names the
   culprit — site, network, Lumen — only with evidence, with three confidence levels («established»,
   «likely», «unknown»). Its own fault is stated plainly. Site banners «unsupported browser» are not
   trusted at face value.
7. **Breakage reports outside telemetry.** A report is an explicit one-off action with a preview; the
   URL is trimmed to `scheme://host/path`; no client id; 30-day retention; receiver reachable from
   Russia; raw reports are never published (the pack, the KB and aggregates are). Passive breakage
   signals (repeated reloads, white screen, challenge loops) stay local; an opt-in aggregate without
   identifiers is allowed from stage 2 with a k-anonymity threshold for publication.
8. **Privacy that does not break login and payment** (amends ADR-007 Layers 3–4 for the Standard
   profile):
   - «like the crowd», not noise: stable plausible values; canvas noise (if kept) keyed by
     *profile + site*, and none of noise or blocking on challenge, bank and government domains listed
     in the pack;
   - real Windows language (`Accept-Language`, `navigator.languages`) and time zone in the Personal and
     Work profiles; `en-US` and UTC only in Anonymous and Tor;
   - cookies without `SameSite` default to `None` (Firefox/Safari) until the 3-D Secure corpus passes,
     then revisit Lax + Lax-POST;
   - transient user activation of 5 s for popups (OAuth, 3-D Secure), keeping `opener`;
   - a content blocker on by default only after: an unbreak list for critical flows, SDK shims
     (including VK ID, Yandex ID, Rambler ID, Sber ID, Metrica), tab context in requests, and
     per-site exceptions persisted for 30 days. «Protection» and «engine» breakage are measured
     separately.
9. **DRM.** Honest absence is the product state until a CDM exists (no EME exposed, a clear «needs
   Widevine» panel with hand-off). The Widevine licence application is filed in month 1 (owner: the
   user). PlayReady via Media Foundation after MSE (it does not cover Russian streaming services).
10. **Electronic signature and GOST.** v1 explains and hands off (Yandex Browser / Chromium-Gost).
    Stage 3: a built-in bridge to the native-messaging hosts of CryptoPro, the Gosuslugi plugin and
    Rutoken (their Chrome extension ids are hard-coded in vendor scripts); GOST-TLS through SSPI only
    by explicit choice with CryptoPro CSP 5. No full WebExtensions just for signing; do not compete
    for the ЕИС/treasury niche in v1.
11. **Bug-compat policy.** Chrome and Safari agree → behave like them; Chrome-only but on > 1 % of
    page loads → implement; rare → a per-domain `engine_flag` from the pack. Do not patch V8
    behaviour «towards the spec» where V8 matches Chrome.
12. **Readiness is measured by end-to-end scenarios.** Release gates (alpha → beta → wide) in
    [`docs/plan/phases.md`](../plan/phases.md) Phase 3 use weighted scenario pass rate, parity with
    Chrome on a corpus measured from inside Russia and abroad, anti-bot pass rate and stability.
    WPT remains the per-subsystem regression net. Bugs found on the compat corpus are prioritized by
    the impact axis in [`docs/bug-priority.md`](../bug-priority.md).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep ADR-007's honest `Lumen/0.x` as the only UA | Hundreds of per-site overrides, a sniffing wave like Firefox OS's new token got (incl. Russian sites), `alfabank.ru` answers 403 to an unknown token; Servo and pre-2019 Vivaldi were locked out of WhatsApp Web, Teams, LinkedIn |
| Full Chrome mask with no token, forever | Lumen disappears from site analytics (nobody will ever test for it), formally breaks Google's sign-in criterion, contradicts «transparent»; accepted only as the starting point before the A/B |
| Leave the current state | Contradiction across 4–5 layers is the strongest anti-bot signal — strictly worse than either consistent option |
| Trust НУЦ out of the box (Yandex model) or not at all | Out of the box = trusting a state CA without consent; not at all = no Russian banks. Consent + CT is the private option and an honest advantage («banks without a state root in the OS») |
| Embedded fallback engine (WebView2/CEF, IE-mode in reverse) | See Decision 5; multi-engine browsers (Lunascape, 360/QQ) show the second engine only helps a vendor who owns both, and even years of URL lists do not reach 100 % |
| Telemetry-tied breakage reporter (Firefox model) | Exactly the privacy-minded users lose the reporter; contradicts the no-telemetry default |
| Measure readiness by WPT % | Saturates and misleads (Ladybird); misses gatekeepers, which are the first breakage class |

## Consequences

- **Positive:** the cheapest and largest class of breakage (identity, trust, lying APIs) is closed in
  weeks (S–M); interventions become auditable data that ships in hours and is retired when the engine
  catches up; users get an exit instead of a dead end, and the hand-off becomes a breakage sensor;
  the «transparent» positioning turns a practice every browser hides into a visible feature.
- **Negative / trade-offs:** a permanent operational function (compat duty, pack ownership with
  signing keys, external relations, a lab with residential IPs in Russia and test accounts); a
  receiver and a mirror to host; ADR-007's «honest own UA» is postponed until the A/B; privacy
  defaults are relaxed for the Standard profile (real language and time zone).
- **Owner actions outside the code** (tracked as `COMPAT-BIZ-*` rows in `ROADMAP.md`): Widevine
  licence application, Cloudflare Browser Developer Program (join; disclose participation and general
  terms publicly), contacts with Google Accounts/reCAPTCHA, Yandex SmartCaptcha, Ozon/WB/Avito
  anti-bot teams; the legal entity for these contracts (sanctions exposure of Widevine/OpenH264 is
  unverified — decide with a lawyer); the Russian lab (residential IPs of several carriers, test
  Gosuslugi accounts, test e-signature tokens).
- **Revisit when:** the A/B 2×2 is repeated with a JS arm in Lumen (`window.chrome`, device APIs,
  JS challenges; token/brand part done 2026-10-15); the
  network stack is blamed for ≥ 3 % of top-1000 refusals (TLS/H2 project); the 3-D Secure corpus
  passes (SameSite); a CDM licence is granted or refused; Privacy Pass / PACT attestation stabilizes.
