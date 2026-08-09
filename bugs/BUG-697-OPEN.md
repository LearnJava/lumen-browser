# BUG-697 — `FeaturePolicy.allowsFeature()` returns `true` for unrecognized feature names, not just unlisted-but-known ones

**Статус:** OPEN
**Компонент:** js (`crates/js/src/permissions_policy.rs:37-44` — `FeaturePolicy.prototype.allowsFeature`)
**Найден:** P3, BUG-361 fix (2026-08-09), while auditing the same file — pre-existing gap, not introduced by that fix

## Симптом

`document.permissionsPolicy.allowsFeature(name)` returns `true` for *any*
string not present in `_ppStore`, including names that are not registered
Permissions Policy features at all:

```js
document.permissionsPolicy.allowsFeature('made-up-xyz')  // → true (spec requires false)
```

Already noted in [BUG-361](BUG-361-FIXED.md)'s original write-up (§"Масштаб",
`font-access` probe): `allowsFeature('local-fonts')` and
`allowsFeature('made-up-xyz')` both returned `true` on a plain page with no
`Permissions-Policy` header.

## Причина

`allowsFeature` (`permissions_policy.rs:37`) treats "not present in
`_ppStore`" as "default-allow", without checking whether the name is a
feature the user agent actually recognizes:

```js
FeaturePolicy.prototype.allowsFeature = function(feature, origin) {
  var entry = _ppStore[feature];
  if (entry === undefined) { return true; }  // default-allow for unlisted
  ...
```

Per W3C Permissions Policy the correct behavior distinguishes two cases: a
policy-controlled feature *not mentioned* in the document's policy falls
back to its declared default allowlist (`self` for most features — i.e.
effectively "allowed", matching current behavior), but a name the UA does
not recognize as a feature at all is not policy-controlled and must return
`false`.

BUG-361's fix (2026-08-09) added `_ppSupported`, a list of feature names
this engine has a real implementation for, and used it in `features()`
and `allowedFeatures()`. `allowsFeature()` was deliberately left unchanged
in that fix — it's a distinct behavior (permission check, not feature
enumeration) and changing its default-allow semantics risks masking real
Permissions-Policy enforcement gaps behind a differently-shaped bug.

## Возможный фикс (не реализован)

`_ppSupported` alone isn't the right gate for `allowsFeature`: doing
`if (_ppSupported.indexOf(feature) === -1) return false;` would make
*every* registered-but-unimplemented feature (e.g. `camera`, `payment`)
report `false` even though nothing in the spec says an unsupported feature
must be blocked — Phase 0 policy here is default-allow regardless of
implementation status (see file header comment). What's actually needed is
a separate, larger registry of *recognized* Permissions Policy feature
names (the full IANA-style registry, not just `_ppSupported`) so
`allowsFeature` can distinguish "recognized but not explicitly restricted"
(→ true) from "not a real feature name" (→ false). Low priority — no WPT
category is currently blocked purely on this (the two probes above were
incidental, not the crux of a failing test).
