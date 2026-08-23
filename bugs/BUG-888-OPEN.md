# BUG-888 — `document.open()` и `document.close()` отсутствуют целиком (`document.write` при этом есть)

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, вариант `doc-write`)
**Область:** js (`crates/js/src/dom.rs:6212-6240` — блок `document.write`/`writeln`; `open`/`close` в нём не определены)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```
document.open   → undefined
document.close  → undefined
document.write  → function
document.writeln→ function
```

Вызов `document.open()` бросает `TypeError: document.open is not a function`
первой же строкой, поэтому весь набор `dynamic-markup-insertion/opening-the-
input-stream/*` не начинается вовсе.

Поведение `document.write` после `load` — осознанный no-op
([BUG-701](BUG-701-FIXED.md): вместо спекового разрушительного неявного
`document.open()` текст просто отбрасывается), и это подтверждено замером:
`document.write("<p id=w1>")` после `load` не вставляет узел
(`found=false`) и не исполняет записанный `<script>`. Но именно поэтому
`document.open()` нужен как отдельная точка входа: тест, который открывает
поток явно, сейчас не может ни начать, ни проверить замену документа.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant doc-write --variant
doc-open` (2026-08-23, dev-release, Linux, `main` = `0dc60692d`):

```
doc-write  ticks=17  dmi open=undefined close=undefined write=function writeln=function
                     wrote-plain found=false
                     wrote-script-tag
                     doc-write-alive ready=complete
doc-open   ticks=15  docopen-threw TypeError: document.open is not a function
                     docopen-alive found=false
```

`wrote-script-ran` не напечатан — это половина [BUG-568](BUG-568-OPEN.md),
которая после BUG-701 стала следствием сознательного no-op, а не отдельным
дефектом исполнения.

## Цена по WPT

Один id остатка WPT-RUN-5:
`html/webappapis/dynamic-markup-insertion/opening-the-input-stream/document.open-03.html`
(«document.open and no singleton replacement»). Вся папка
`opening-the-input-stream/` (~30 id) не вендорена — цена по остатку нижняя.

## Что дальше

HTML LS §3.2.5 «document.open()»: снести узлы документа, поставить точку
вставки, вернуть тот же `Document`; `close()` — закрыть поток. Реализовывать
имеет смысл вместе с решением по BUG-701: неявный `document.open()` из
`write()` после `load` остаётся отключённым сознательно, а явный вызов должен
работать.
