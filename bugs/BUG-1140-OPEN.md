# BUG-1140 — `srcset`: URL кандидата обрезается на первой запятой

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** html-parser (`crates/engine/html-parser/src/srcset.rs:69` — `while … && bytes[pos] != b','`: сбор URL останавливается на запятой)

## Симптом

amazon: все картинки с запятыми в URL (`…_AC_AIweblab1431263,T1_SF1282.5,2052_QL80_.jpg`)
запрашиваются обрезанными до первой запятой → `← 400` (шесть картинок промо-блока). На apple в логе
`data:image/gif;base64: data: URL missing comma` — вероятно, тот же разрез `data:`-URL в `srcset`
(гипотеза, отдельного репро нет).

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g1/srcset_comma.html`:

```html
<!doctype html><html><body>
<!-- Amazon: srcset-URL содержит запятые (".../51l7ZOsRo7L._AC_AIweblab1431263,T1_SF1282.5,2052_QL80_.jpg 1.5x").
     HTML LS §4.8.4.3.10 шаг «collect a sequence of code points that are not ASCII whitespace» — запятая внутри URL допустима,
     срезаются только завершающие запятые. Сервер покажет, какой URL запросил браузер. -->
<img id=i src="srcset_a,b_default.png" srcset="srcset_a,b,c_1x.png 1x, srcset_a,b,c_2x.png 2x">
<script>
setTimeout(function(){ var i=document.getElementById('i'); window.__r={currentSrc:i.currentSrc, srcset:i.srcset}; console.log('RESULT '+JSON.stringify(window.__r)); },300);
</script>
</body></html>
```

**Результат:** Lumen (`--dump-layout`): `→ GET http://127.0.0.1:8761/srcset_a` → 404. Chrome: `currentSrc='http://127.0.0.1:8761/srcset_a,b,c_1x.png'`.

## Что сделать

HTML LS §4.8.4.3.10 «parse a srcset attribute», шаг 4–6: URL — это последовательность
не-пробельных символов; с конца снимаются только запятые, и если их было больше одной — ошибка
разбора. Запятая внутри URL не разделяет кандидатов. Критерий: репро запрашивает
`srcset_a,b,c_1x.png`; `data:`-URL в `srcset` apple проверить заодно.
