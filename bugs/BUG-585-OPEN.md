# BUG-585: `Origin` WebIDL global (`Origin.from()`) not implemented at all

**Статус:** OPEN (ДОРАБОТКА → GAP-ORIGIN)
**Тип:** ДОРАБОТКА — целиком нереализованная фича (глобальный конструктор,
opaque/tuple-различение происхождения, PSL-based same-site, cross-origin/
sandboxing проверки, Worker-скоуп), не дефект реализованного кода.
Перенесено в [GAP-ORIGIN](../ROADMAP.md).
**Компонент:** js (`crates/js/src/dom.rs` — grep for `\bOrigin\b` outside comments returns zero hits; the only match is a code comment at `dom.rs:5370` about `MessageEvent.origin`, unrelated to the interface)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL Origin.from("https://site.example") is a tuple origin. - Origin is not defined
```

181 occurrences across every file in `html/browsers/origin/api/` (self-contained
category — the one dependency, `resources/serializations.js`, is vendored and
loads fine). Every `origin-from-*.any.js`/`.window.js` test in the directory
fails on the same `ReferenceError`.

## Причина

The `Origin` interface (`Origin.from(value)`, tuple vs. opaque origin,
serialization, comparison — HTML LS `#origin-2`) has no implementation
anywhere in the JS shim: no global constructor, no `.from()` static, no
`.opaque`/`.serialize()` members. Distinct from the many already-implemented
places that compute an origin internally (URL parsing, `postMessage`,
`document.domain`) — none of them expose the constructible `Origin` object
the tests instantiate.

## Масштаб

Whole feature, entirely within `html/browsers/origin/api/`. Knock-on: 25 of
the 181 failures are `origin-from-hyperlinkelementutils.window.js` cases that
also trip `Cannot set properties of undefined (setting 'baseVal')` on SVG
`<a href>`/`<a xlink:href>` — `SVGAnimatedString.baseVal` for the `href`
attribute is a second, smaller gap the tests would hit right after `Origin`
is fixed.

## Ревизия P3 2026-09-15

Реклассифицировано в ДОРАБОТКУ, тот же класс, что BUG-553/554/562/568/583/584:
целиком отсутствующая фича, не точечный дефект. Разведка (чтение исходников
шима + всех 14 тестовых файлов `html/browsers/origin/api/`) показала не одну
недостающую привязку, а шесть переплетённых пробелов:

1. Самого понятия opaque-происхождения в JS-слое нет: `_lumen_parse_url`
   (`crates/js/src/shim/url_parse_shim.js:43`), единственный источник
   `URL.prototype.origin`/`Location.prototype.origin`, всегда возвращает
   непустую строку `protocol + '//' + host`, даже для `file:`/`data:`/
   `blob:`. Верное opaque/tuple-различение существует только на Rust-стороне
   (`origin_for_url`, `crates/js/src/file_input.rs:82-100`) и в JS не
   проброшено — `.opaque` не на чем построить без переделки этой части.
2. `.isSameSite()` требует PSL-aware registrable domain. `PslProvider`
   (`crates/storage/src/psl.rs`) недостижим из `crates/js`: `crates/storage`
   уже зависит от `lumen-js` (обратная связь дала бы цикл), значит нужен
   новый нативный биндинг тем же DI-паттерном, что `AudioPlaybackProvider`
   (`crates/js/src/audio_element.rs:44-84`, регистрация
   `rt.register_native`, шелл-обвязка `crates/shell/src/window_mode.rs:62`)
   — не JS-only правка.
3. `Origin.from(Window)`/`Origin.from(MessageEvent)` обязаны бросать
   `TypeError` на cross-origin/sandboxed фреймах и окнах
   (`origin-from-window.window.js`, `origin-from-messageevent.window.js`) —
   упирается в модель same-origin-domain/sandboxing применительно к этому
   API, которой пока нет.
4. Конструированные `MessageEvent`/`ExtendableMessageEvent` обязаны давать
   `Origin.from()` бросить (`origin-from-messageevent.window.js:4-7`,
   `origin-from-extendablemessageevent.any.js:15-18`), а полученные через
   реальную доставку — нет: нужен внутренний слот «настоящего» origin
   отдельно от видимого `.origin`.
5. `Origin` обязан работать и в Worker-скоупе (`origin-from-worker.window.js`,
   `origin-from-extendablemessageevent.any.js`) — установка в оба шима
   (`web_api_shim()`/`worker_exposed_shim()`, `crates/js/src/dom.rs:496-519`).
6. `Origin.from(HyperlinkElementUtils)` цепляет уже отдельно заявленный
   пробел `SVGAnimatedString.baseVal` на `<a href>`/`xlink:href`
   (`origin-from-hyperlinkelementutils.window.js`), см. «Масштаб» выше.

Пункты 1 и 5-6 реализуемы в JS-шиме; пункт 2 требует нового нативного
биндинга; пункты 3-4 требуют модели browsing-context изоляции, которой в
движке пока нет вообще — реализация одного лишь `Origin.from(string|URL)`
покрыла бы меньшую часть из 181 сабтеста и оставила бы остальные падать с
менее понятной картиной, чем сейчас. Перенесено в
[GAP-ORIGIN](../ROADMAP.md).
