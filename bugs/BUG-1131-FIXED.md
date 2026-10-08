# BUG-1131 — Нет глобального интерфейса `IntersectionObserverEntry`

**Статус:** FIXED 2026-09-26 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b3.js:1479` `function IntersectionObserver`, `:1608` — запись собирается литералом `{isIntersecting:…, intersectionRatio:…}`; экспорт — `web_api_shim_tail_mc.js:48`)

## Симптом

duolingo: инлайн-скрипт проверки поддержки —
`"IntersectionObserver"in window&&"IntersectionObserverEntry"in window&&"intersectionRatio"in
IntersectionObserverEntry.prototype&&"isIntersecting"in …` → `false` →
`window.location.href="/errors/not-supported.html"` → `← 404` → «Ошибка загрузки» (BUG-1114).
Остальные 6 проверок (ES2019, WebAssembly, AbortController+Request.signal, animate,
ResizeObserver) в Lumen проходят.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g1/ioentry.html`:

```html
<!doctype html><html><body><script>
var r={};
r.IO=typeof IntersectionObserver; r.IOE=typeof IntersectionObserverEntry;
r.IOE_in_window=("IntersectionObserverEntry" in window);
try{var p=window.IntersectionObserverEntry&&window.IntersectionObserverEntry.prototype;
 r.proto_keys=p?Object.getOwnPropertyNames(p).join(','):null;
 r.intersectionRatio_in=p?("intersectionRatio" in p):null; r.isIntersecting_in=p?("isIntersecting" in p):null;}catch(e){r.err=String(e);}
// точная проверка Duolingo (index.html, inline) → при false делает location.href="/errors/not-supported.html"
r.duolingo_check=("IntersectionObserver"in window&&"IntersectionObserverEntry"in window&&"intersectionRatio"in window.IntersectionObserverEntry.prototype&&"isIntersecting"in window.IntersectionObserverEntry.prototype);
console.log("RESULT "+JSON.stringify(r)); window.__r=r;
</script></body></html>
```

**Результат:** Lumen: `IO=function`, `IOE=undefined`, `IOE_in_window=false`, `duolingo_check=false`. Chrome: `IOE=function`, на prototype `time`, `rootBounds`, `boundingClientRect`, `intersectionRect`, `isIntersecting`, `isVisible`, `intersectionRatio`, `target`; `duolingo_check=true`.

## Что сделать

Intersection Observer §2.3: интерфейс `IntersectionObserverEntry` с конструктором и
readonly-атрибутами на prototype; записи наблюдателя создавать через него. Критерий: репро даёт
`duolingo_check=true`; duolingo перемерить.

## Исправление (2026-09-26, P6)

`web_api_shim_mid_b4.js`: интерфейс `IntersectionObserverEntry` рядом с
`_io_dom_rect` — конструктор по `IntersectionObserverEntryInit` (обязательные
члены → `TypeError`, `target` — Element, прямоугольники через
`DOMRectReadOnly.fromRect`), семь readonly-геттеров на prototype (`get <attr>`),
поля — в неперечислимом слоте `_ioe`, `Symbol.toStringTag`.
`_lumen_deliver_intersection_observers` создаёт записи
`Object.create(IntersectionObserverEntry.prototype)` + `_io_entry_slot`, а
`_io_dom_rect` возвращает `DOMRectReadOnly` вместо литерала. Экспорт —
`web_api_shim_tail_mc.js`.

Проверка: репро-проверка duolingo на `--dump-layout` — `true` (сборка `main` до
фикса — `false`). Тесты `dom::tests::v8_bug1131_io_entry` (3). WPT
`intersection-observer`: 162/383 → 174/383 сабтестов, регрессий нет
([заметка](../docs/wpt-vendor-notes/intersection-observer.md)).
Живой duolingo не перемерялся (нужно окно с сетью).

Остаток вне скоупа: глобал enumerable (общая черта экспорта шима),
`IntersectionObserverEntry.prototype` writable, `isVisible` (IO v2).
