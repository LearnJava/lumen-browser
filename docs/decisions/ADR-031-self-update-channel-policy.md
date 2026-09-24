# ADR-031: Self-update channel — trust, cadence and privacy posture

## Status

Accepted

## Date

2026-09-25

## Context

UPD-1..10 (`ROADMAP.md`, `crates/shell/src/update.rs`, `crates/update-manifest`)
built the self-update pipeline end to end: signed `latest.json` manifest, a
background checker, staged download + SHA-256 verify, atomic binary swap, and
a `#updateBar` UI surface. Every mechanical piece shipped with an inline
comment explaining its own choice, but no single document states the
*policy* the pieces jointly implement, and `CLAUDE.md`'s "docs move with the
code" rule has nothing to point at for a reviewer or a future contributor
asking "why does the checker never send cookies" or "why 24 hours and not
1 hour". UPD-11 exists to close that gap — record the decision once, in the
place `docs/plan/privacy.md` §9.6 ("No silent network") already promises it
would be documented.

Four questions recur whenever self-update code is touched and none had a
written answer before this ADR:

1. **Channel** — which URL, and is it the only one Lumen calls without a
   user action?
2. **Signature** — who is trusted to publish an update, and what happens to
   an untrusted manifest?
3. **Data on the wire** — does the check leak anything identifying?
4. **Cadence and consent** — how often, and can the user turn it off?

## Decision

**Self-update is the one exception to "no phone-home" (`docs/plan/privacy.md`
§9.7), and it is deliberately built to leak as little as a version check can.**

1. **Channel.** The checker fetches exactly one stable URL —
   `https://github.com/…/releases/latest/download/latest.json`
   (`update::MANIFEST_URL`) — chosen over the GitHub REST API specifically to
   avoid the API's 60 req/h/IP rate limit and its request headers/auth
   surface. No other endpoint is contacted by the update subsystem. This is
   the *only* unconditional outbound request family Lumen makes without a
   user-initiated navigation or an explicit user action elsewhere in the
   browser.

2. **Signature.** A manifest is worthless without ed25519 verification
   against a hardcoded allow-list (`TRUSTED_KEYS`, `lumen-update-manifest`),
   checked with the *same* `signing_body` function the release signer uses
   (`crates/update-manifest` exists specifically so the two never drift —
   see `subsystems/update-manifest.md`). `apply_check_result` rejects an
   unverified or tampered manifest before it ever reaches a caller:
   `CheckOutcome::Available` is only ever produced from a manifest that
   verified. A verification failure is logged distinctly from "no update"
   so a live corruption or spoofing attempt is visible, but is otherwise
   treated exactly like "up to date" — it never surfaces a raw, untrusted
   manifest to the user or to any caller.

3. **Data on the wire.** The request is a conditional GET only: `ETag`/
   `Last-Modified` validators are round-tripped so an unchanged manifest
   costs a `304`, and no cookie jar, no client identifier, no telemetry
   payload, no user-agent fingerprinting beyond what the HTTP transport
   already sends for any request is attached. The manifest itself carries
   no per-installation token — it is one static file, identical for every
   Lumen instance in the world at a given release. This mirrors GPC's
   design goal in ADR-026: the mechanism should not itself become a
   tracking signal.

4. **Cadence and consent.** Automatic checks are throttled to at most once
   per `CHECK_INTERVAL_SECS` (24 h) and run only when
   `UpdateState::auto_check_updates` is true — **on by default**, because a
   privacy browser withholding security fixes by default is a worse
   trade-off than one conditional-GET a day, but with a single, honest,
   reachable opt-out: the toggle in Settings → General → «Обновления». A
   manual «Проверить сейчас» (`check_for_update_now`) bypasses the
   throttle — an explicit click is its own consent, and unlike the
   automatic path it surfaces a network error as `Err` instead of folding
   it into "up to date", because a manual check silently claiming
   freshness when it never reached the server would be a lie the user
   asked not to be told. Both paths are skipped entirely for
   `no_persistent_state` sessions (BiDi/MCP automation, Tor) — the existing
   "don't touch disk/network for automation and Tor sessions" principle
   already applied to the HTTP cache in the same call site
   (`cli_args::run_cli`) — because those sessions have no `data/` to persist
   a throttle timestamp into and no business making an out-of-band request
   for a scenario that explicitly asked for zero footprint.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| No self-update at all, users update manually from GitHub Releases | Real users don't; a privacy browser that silently falls behind on security patches trades one risk for a worse one. Rejected by the original UPD-1 brief, not revisited here. |
| Telemetry-style "phone home with hardware ID" update check (common in commercial browsers) | Directly violates `docs/plan/privacy.md` §9.1/§9.7 — the whole point of Lumen is that nothing identifies the installation. A static manifest file with no query parameters achieves the same freshness check with zero identifying surface. |
| Poll the GitHub REST API (`api.github.com/repos/…/releases/latest`) | Same freshness information, but the API's 60 req/h/IP rate limit is shared across every unauthenticated caller from that IP — a stable static-file URL under `/releases/latest/download/` has no such limit and needs no auth token. |
| Auto-check off by default, opt-in only | Users overwhelmingly never opt in to anything, and unpatched security bugs are the actual harm being weighed against one conditional GET/24h with zero identifying payload. On-by-default with an honest, one-click opt-out was judged the better trade for a *browser*, as distinct from e.g. crash-report telemetry (`docs/plan/privacy.md` §9.8, which stays opt-out-proof and local-only because it can carry arbitrary user data). |
| Shorter interval (e.g. hourly) for faster patch propagation | No real gain — GitHub Releases do not publish security fixes on an hourly cadence, so a 24h check catches every release within one day of publication at 1/24th the request volume. Matches the EasyList-politeness precedent already used elsewhere in the codebase for periodic background fetches. |
| Silently ignore an unverified manifest without logging anything | Makes a live spoofing/corruption attempt against the update channel invisible even to someone looking at logs. Distinct logging costs nothing and preserves the "no silent network" transparency principle for the one case where silence would hide an actual attack, not just noise. |

## Consequences

- **Positive:** the four recurring questions above now have one canonical,
  linkable answer instead of being re-derived from source comments each
  time; `docs/plan/privacy.md` §9.6's "phone-home... can be disabled" claim
  is now backed by a decision record, not just a UI toggle nobody wrote down
  the rationale for. `subsystems/shell.md`/`subsystems/storage.md` gained a
  pointer to this ADR instead of re-stating the policy inline.
- **Negative / trade-offs:** self-update remains the one code path in Lumen
  that makes an unconditional outbound request without a user-initiated
  navigation, which is a real (if minimal) exception to the "no silent
  network" default and must be re-justified, not silently extended, if a
  future task wants to attach anything beyond version/ETag to the request.
- **Future:** if Lumen ever ships a beta/nightly channel, this ADR is the
  place to extend — a second manifest URL is a policy decision of the same
  shape (trust, cadence, data) as this one, not a free-standing feature.
  `TRUSTED_KEYS` rotation (adding a new signing key without invalidating the
  old one immediately) is already supported by the map shape but not yet
  exercised in practice; that remains out of scope until a real key
  rotation is needed.
