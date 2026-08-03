# BUG-544: `Element.prototype.animate`/`getAnimations` don't exist — Web Animations API is wired as own-instance properties, so `'animate' in Element.prototype` (a standard WPT feature-detection idiom) always reports `false`

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs:6602-6607` — `_lumen_make_element` element factory)
**Найден:** WPT-RUN-3 срез 31 (`ROADMAP.md`) — массовый прогон `css/css-transforms`+`css/css-fonts`+`css/css-ui`+`css/css-text`+`css/css-flexbox`

## Механизм

Every DOM element object is built by a per-node factory function
(`_lumen_make_element`). `Element.prototype.animate`/`getAnimations` are
never assigned — instead the factory does, per instance:

```js
// dom.rs:6602-6607
_obj.animate = function(keyframes, options) {
    return _wa_element_animate(this, keyframes, options);
};
_obj.getAnimations = function() {
    return _wa_get_animations_for(this);
};
```

This is an **own property of `_obj`**, set fresh on every element
construction — never on the shared `Element.prototype`. Calling
`el.animate(...)` on an actual element works fine (own property lookup
finds it), so the method is *functionally* present. But any code that
feature-detects via the interface's prototype instead of an instance —
`'animate' in Element.prototype` — walks the prototype chain and never
finds it, since `Element.prototype` itself has no such property. The
result: `'animate' in Element.prototype` is unconditionally `false`, even
though `document.createElement('div').animate` works.

## Симптом

`css/support/interpolation-testcommon.js` — the shared helper `test_interpolation()`
(used by essentially every per-property CSS value interpolation test across
the whole WPT CSS corpus, not just this slice's five categories) gates its
entire "Web Animations" interpolation method on:

```js
// interpolation-testcommon.js:178
isSupported: function() {return 'animate' in Element.prototype;},
```

Since this is `false`, every `test_interpolation()` call fails its
`assert_true(interpolationMethod.isSupported(), 'Web Animations should be
supported')` check before ever calling `target.animate(...)` — masking
whatever the actual per-property interpolation behavior would otherwise
show. This slice alone: **2207 subtests / 52 files** hit this exact
assertion, plus **737 subtests / 6 files** on the sibling
`assert_true(CSS.supports(property, from), "'from' value should be
supported")` checks in the same helper (gated the same way once the first
assertion already failed) — the single largest failure cluster of the
slice, spanning `css-fonts`, `css-transforms`, `css-ui`, `css-text`, and
`css-flexbox` simultaneously. Given `interpolation-testcommon.js` is shared
verbatim across the entire CSS corpus (already seen driving part of
[BUG-536](BUG-536-OPEN.md)'s CSS-Transitions-specific findings), this gap
is expected to recur in every future WPT-RUN-3 slice that touches a CSS
value's interpolation tests, not just these five categories.

## Как исправить (не входит в объём P2)

Move the `animate`/`getAnimations` assignment from the per-instance
`_obj.animate = ...`/`_obj.getAnimations = ...` in `_lumen_make_element`
onto `Element.prototype.animate`/`Element.prototype.getAnimations` (both
already reference `this`, so no behavior changes beyond where the property
lives — `this` still binds to the calling element either way). Worth
auditing whether other WAAPI-Level-1-adjacent methods added the same way
elsewhere in `_lumen_make_element` have the identical gap before treating
this as fully closed.

## Срез 33 (`css/css-sizing`, 2026-08-03)

Largest single-slice contribution yet: 565 subtests across 10 files, all in
`animation/*-interpolation.html` + `contain-intrinsic-size/animation/*`
(the CSS Values interpolation test suite drives every case through
`element.animate([...])`/`getAnimations()` before sampling). `.ini` under
`tests/wpt/metadata/css/css-sizing/animation/` and
`tests/wpt/metadata/css/css-sizing/contain-intrinsic-size/animation/`.
