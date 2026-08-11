# ADR-026: Global Privacy Control is a property of the HTTP profile, not a separate toggle

## Status

Accepted

## Date

2026-08-11

## Context

Global Privacy Control ([W3C GPC](https://www.w3.org/TR/gpc/)) is a universal
opt-out signal with two halves that a site can observe independently:

* the `Sec-GPC: 1` request header, and
* the `navigator.globalPrivacyControl` boolean.

Unlike DNT, GPC is legally binding in several US jurisdictions (CCPA/CPRA,
the Colorado Privacy Act, the Connecticut Data Privacy Act all recognise
`Sec-GPC: 1` as a valid opt-out request), so it is a signal Lumen — a
privacy-first browser — has a positive reason to send.

[BUG-397](../../bugs/BUG-397-FIXED.md) found neither half implemented. Adding
them raised two questions the bug report explicitly deferred to a decision:

1. **Which profiles send it?** Lumen's outgoing fingerprint is chosen by
   `HttpProfile` (ADR-007): four profiles impersonate a real browser
   byte-for-byte (`Chrome`/`Edge`/`Safari`/`Firefox`), one imitates Tor
   Browser, and two state Lumen's own position (`Lumen`, `Strict`).
2. **Is there a user-facing toggle?** `about:settings` already has a Privacy
   tab, so a per-browser GPC switch is a natural-looking feature.

The constraint that decides both: the two halves must never disagree. A page
that sees `Sec-GPC: 1` on the wire but no `navigator.globalPrivacyControl` (or
the reverse) has learned something no real browser would tell it — the
contradiction is a fingerprinting bit in its own right, which defeats the point
of the privacy signal carrying it.

## Decision

**GPC is derived from the HTTP profile and has no independent control.**

A single predicate, `lumen_network::sends_global_privacy_control(HttpProfile)`,
is the only source of truth. Both halves are wired from it: the network layer
emits the header under it, and the shell passes the same value to
`lumen_js::set_global_privacy_control`, which gates the JS property.

It returns `true` for exactly `HttpProfile::Lumen` and `HttpProfile::Strict` —
the two profiles that state Lumen's own privacy position rather than impersonate
another browser. It returns `false` for the impersonation profiles, because
real Chrome/Edge/Safari have no native GPC and Firefox ships it off in normal
browsing: sending the header there would be a tell, not privacy.

`HttpProfile::TorBrowser` is `false` **pending verification**. Tor Browser
inherits Firefox's "on in private browsing" behaviour in principle, but this
was not verified against a real Tor Browser build, and a wrong guess breaks the
byte-exact header match that profile exists for.

When the signal is off, `navigator.globalPrivacyControl` is **absent**, not
`false`: `'globalPrivacyControl' in navigator` is exactly the check a
fingerprinting script runs, and `false` would mean "supports GPC, user turned it
off" — a bit of entropy the impersonated browser does not emit (same class as
[BUG-379](../../bugs/BUG-379-FIXED.md)).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| A separate `about:settings` toggle, independent of the profile | Immediately recreates the disagreement this decision exists to prevent: GPC on + `http_profile = "chrome"` sends a header real Chrome never sends, making the user *more* identifiable while they believe they raised their privacy. The toggle's only honest implementation is "switch to a privacy profile", which the profile selector already is. |
| Send `Sec-GPC: 1` on every profile, unconditionally | Breaks the byte-exact impersonation the four mimicry profiles exist for — the header would single Lumen out of the crowd it is hiding in. Wrong trade even though GPC itself is desirable. |
| Expose `navigator.globalPrivacyControl = false` when off | Reveals GPC support, which the impersonated browsers do not have. Absence is the only value that matches them. |
| Two independent implementations (header in network, property in JS), kept in sync by convention | The sync is the whole requirement; leaving it to convention makes drift a matter of time. A shared predicate makes agreement structural, and an exhaustive per-profile test makes silent drift a test failure. |
| Per-site GPC control | GPC is a *universal* opt-out by design (W3C GPC §2). Per-site granularity contradicts the spec's purpose and multiplies the fingerprint surface. |

## Consequences

- **Positive:** the two halves cannot disagree by construction, not by
  discipline; a per-profile test asserts header presence equals the predicate
  for all seven profiles, so adding a profile without deciding its GPC stance
  fails the suite. Users get a legally-recognised opt-out by choosing
  `strict`/`lumen`, with no new UI surface to explain or maintain.
- **Negative / trade-offs:** a user who wants GPC *and* Chrome impersonation
  cannot have both. This is deliberate — the combination is self-defeating —
  but it is a real capability refused. The signal is also invisible in
  `about:settings`: its state is only inferable from the fingerprint-mode row.
- **Future:** revisit `HttpProfile::TorBrowser` once its behaviour can be
  checked against a real Tor Browser build (the condition is recorded in the
  predicate's doc comment). If Lumen ever gains profiles that are neither
  impersonation nor own-identity, the predicate — not a scattered set of
  `match` arms — is the one place to extend.
