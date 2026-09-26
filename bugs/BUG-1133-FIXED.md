# BUG-1133 — `atob` не принимает base64 без паддинга и принимает `=` в середине (не forgiving-base64)

**Статус:** FIXED 2026-09-26 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** js (`crates/js/src/shim/web_api_shim_mid_c.js:17-39` — проверка `s.length % 4 !== 0` и `=` в любой позиции; сверить `worker.rs:2514` `atob_native_v8`)

## Симптом

airbnb `https://d0a7e.airbnb.com/tags.js`: таблица строк `v=[…]` — 186 строк base64, из них 74
с длиной ≡2 mod 4 и 51 с ≡3 (например `atob('Y2VpbA')`). 12× `Uncaught InvalidCharacterError: atob:
invalid base64 string at atob (…:20779:15) at a (<anonymous>:2:737)`; `wt is not a function` дальше —
вероятно, следствие. BUG-1016 (FIXED) касался только типа исключения.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/api.html`:

```html
<!doctype html>
<html><head><meta charset="utf-8"><title>g5 api repro</title></head>
<body>
<div id="d" class="a b c">x</div>
<img id="svgc" src="comment.svg" width="18" height="18">
<img id="svgp" src="plain.svg" width="18" height="18">
<script>
function t(f){try{return f();}catch(e){return 'THROW '+(e&&e.name)+': '+(e&&e.message);}}
var d=document.getElementById('d');
window.R={
 atob_unpadded2: t(function(){return atob('YQ');}),
 atob_unpadded3: t(function(){return atob('YWI');}),
 atob_padded: t(function(){return atob('YQ==');}),
 atob_len1mod4: t(function(){return atob('YWJjZ');}),
 atob_eq_middle: t(function(){return atob('YQ==YQ==');}),
 cl_values: t(function(){return typeof d.classList.values;}),
 cl_entries: t(function(){return typeof d.classList.entries;}),
 cl_keys: t(function(){return typeof d.classList.keys;}),
 cl_iter: t(function(){return typeof d.classList[Symbol.iterator];}),
 cl_spread: t(function(){return [].concat(Array.from(d.classList)).join(',');}),
 cl_values_call: t(function(){return Array.from(d.classList.values()).join(',');}),
 javaEnabled: t(function(){return typeof navigator.javaEnabled;}),
 javaEnabled_call: t(function(){return navigator.javaEnabled();}),
 getBattery: t(function(){return typeof navigator.getBattery;}),
};
</script>
</body></html>
```

**Результат:** Lumen: `atob_unpadded2/3 = THROW`, `atob_eq_middle = 'aa'`. Chrome: `'a'`/`'ab'`, `eq_middle = THROW`, `atob('YWJjZ')` — `InvalidCharacterError`.

## Что сделать

HTML LS §8.3 `atob` → Infra «forgiving-base64 decode»: убрать ASCII-пробелы; если длина
кратна 4 — снять 1–2 завершающих `=`; после этого длина ≡1 mod 4 или символ вне алфавита (в том
числе `=` в середине) → ошибка. Один алгоритм для окна и воркеров. Критерий: репро даёт результат
Chrome.

## Исправление (2026-09-26, P6)

Оба `atob` — Infra «forgiving-base64 decode»: снять ASCII-пробелы (TAB/LF/FF/CR/SPACE); если
длина кратна 4 — снять 1–2 завершающих `=`; длина ≡1 mod 4 или символ вне алфавита (в том числе
`=` не в хвосте) → `InvalidCharacterError`; лишние младшие биты последней группы отбрасываются.

- Окно — [`crates/js/src/shim/web_api_shim_mid_c.js`](../crates/js/src/shim/web_api_shim_mid_c.js) `function atob`.
- Dedicated/shared-воркеры — [`crates/js/src/worker.rs`](../crates/js/src/worker.rs) `b64_decode`
  (тот же декодер обслуживает `data:`-URL скрипта воркера — Fetch тоже берёт forgiving-base64).
  Попутно `atob_native_v8` отдаёт двоичную строку (байт → Latin-1 символ), а не UTF-8:
  раньше `atob(btoa('\xff'))` в воркере бросал.

Тесты: `dom::tests::v8_url_abort_clone_blob::atob_is_forgiving_base64` (значения из репро и
Chrome), `worker::tests::b64_decode_is_forgiving_base64`, расширенный
`worker::tests_v8::v8_worker_globals_have_atob_btoa`. WPT `html/webappapis/atob`:
308 неожиданных PASS, 0 регрессий → 760/760, baseline `base64.any.js.ini` удалён как чистый.

Service Worker держит свои `atob`/`btoa` (UTF-8, без `DOMException`) — отдельный
[BUG-1193](BUG-1193-OPEN.md).
