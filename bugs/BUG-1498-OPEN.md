# BUG-1498 — Выбор контейнера для `@container`: вложенный `inline-size` не пропускается при запросе по высоте, `display:none`/`contents`/top-layer/таблица неверно участвуют

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/box_tree/container_anchor.rs` — выбор контейнера по `container-type`/имени/осям)

## Симптом

Запрос по оси, которую ближайший контейнер не отдаёт (`container-type:inline-size` и запрос `min-height`), должен идти к следующему предку, умеющему эту ось; у нас запрос ложен. Затем отдельные случаи «контейнер без principal box»: `display:contents`, `display:none`, контейнер внутри multicol, диалог в top layer, `display:table` (непригоден для containment). Измерено на вложенном контейнере; остальные — по именам файлов и первому сообщению сабтеста.

## Проба

Проба (`--mcp`, `.c{container-type:size;width:200px;height:100px}`, внутри `.d{container-type:inline-size}`, запрос `@container (min-height:50px){#t{color:green}}`):

| случай | у нас | ожидается |
|---|---|---|
| `#t` внутри `.d` внутри `.c`, запрос `(min-height:50px)` | `red` | `green` (`.d` не отдаёт высоту → `.c`) |
| `.c{container-type:inline-size}`, запрос `(min-height:50px)` | `red` | `red` |
| `.c{display:contents;container-type:inline-size}`, `(min-width:1px)` | `red` | `red` |
| `#t{container-type:size}` — запрос к самому себе | `red` | `red` |

## Как найдено

WPT-RUN-14 срез 22: `container-queries/{container-selection,container-nested,nested-query-containers,query-container-name(-dynamic),unsupported-axis,display-none,display-contents,ineligible-containment,top-layer-dialog(-container),size-container-no-principal-box,container-inside-multicol-with-table}` — 13 id, 90 из 129 сабтестов.

## Что делать

Выбирать ближайшего предка, чей `container-type` отдаёт нужную ось (и, при указанном имени, совпадает по `container-name`); учесть eligibility по `display` и top layer.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/container-selection.html`, `unsupported-axis.html`.
