# BUG-1846 — grid: дорожка `auto`/`minmax(0,auto)` получает 0, если в шаблоне есть дорожка `fr` — на portal.nextera.ru невиден заголовок чата и схлопнуты кнопки поля ввода

**Статус:** OPEN (сайзинг-часть ИСПРАВЛЕНА 2026-11-27, ветка `bug-1853-fix`: fr-шаблоны
приняты новым content-сайзером `grid_auto_cols` (§12.4→§12.6/12.7 с заморозкой), фолбэк
`grid.rs` тоже перестал обнулять auto при fr; заголовок чата виден, совпадает с Chrome.
Остаток — размещение `chat-input-row`/`chat-input-bottom` по дорожкам (auto-placement)
и сериализация computed — см. «Срез 2026-11-27»)
**Заведён:** 2026-11-26 (ZCode, portal.nextera.ru, сравнением с Chrome 158.0.8127.37)
**Область:** layout (grid, размерирование дорожек; файл не локализован — область
[`box_tree/grid.rs`](../crates/engine/layout/src/box_tree), `grid_auto_cols.rs`, `intrinsic.rs`,
см. BUG-1394; сериализация computed — [`grid_computed.rs`](../crates/engine/layout/src/grid_computed.rs))

## Симптом на живой странице

`div.pp-chat-head` — `display: grid; grid-template-columns: minmax(max-content,1fr) minmax(0,auto) minmax(max-content,1fr)`;
`div.chat-input-box` — `auto auto minmax(0,1fr) auto auto auto auto`:

| | Chrome 158 | Lumen a0666c3564+патч |
|---|---|---|
| `.pp-chat-head`, дорожки | `736.77px 70.45px 736.78px`, заголовок чата (имя агента) виден | `790px 0px 790px`, `div.pp-head-title` 0×33 — заголовка нет |
| `.chat-input-box`, дорожки | `44px 0px 616px 40px 40px 0px 48px` | `0px 0px 788px 0px 0px 0px 0px` — кнопки поля ввода схлопнуты |
| computed `grid-template-columns` | разрешён в px | на живой странице отдаёт шаблон, не px |

## Локализация условия (синтетическая проба, видимая область, 600px-контейнер)

Полностью разложившийся повтор (двухфазный замер: вставка → ≥1 с → замер; см. ловушку метода —
свежие узлы в Lumen синхронно не меряются):

| шаблон | Lumen (средняя дорожка) | Chrome | верно? |
|---|---|---|---|
| `minmax(max-content,1fr) minmax(0,auto) minmax(max-content,1fr)` | `0` (дети `[292,0,292]`) | `117.55` (`[233.22,117.55,233.23]`) | нет |
| `1fr minmax(0,auto) 1fr` | `0` (`[292,0,292]`) | `117.55` | нет |
| `minmax(max-content,1fr) auto minmax(max-content,1fr)` | `0` | `117.55` | нет |
| `auto auto minmax(0,1fr) auto auto auto auto` | `0` (`[0,0,552]`) | `117.55` (`[23.94,117.55,410.52,0,0,0,0]`) | нет |
| `100px minmax(0,auto) 100px`, `100px auto 100px` | `384` | `384` | да |
| `auto auto auto` | `254.04` (`[160.44,254.04,169.53]`) | `254.05` | да |
| `minmax(max-content,1fr) min-content minmax(max-content,1fr)` | `49.66` | `49.67` | да |

Условие: **дорожка с `auto`-размерированием обнуляется тогда и только тогда, когда в шаблоне есть
дорожка `fr`** — гибкие дорожки забирают всё свободное место, не оставив `auto`-дорожкам
контентного вклада (по CSS Grid: не-гибкие дорожки размерятся до распределения `fr`).

## Что делать

Порядок размерирования: сначала не-гибкие дорожки (`auto` → max-content), затем распределение
остатка по `fr`; в `resolved_track_list` отдавать использованные значения. Свежая раскладка
вставленных узлов — отдельная ловушка метода, к этому дефекту не относится.

## Как проверить

Проба выше (двухфазно); на портале — заголовок чата виден, кнопки поля ввода своих ширин.
Смежный: BUG-1394 (subgrid, другой аспект той же области).

## Срез 2026-11-26 вечер (Lumen 9b4e0a6131 vs Chrome 158, тот же аккаунт портала)

Композер чата зажат примерно вдвое: `.chat-input-area` 429px (flex `0 0 auto`, block) →
грид `.chat-input-box` 397px (`max-width: 820px`) → `#chatTextarea` 200px. В Chrome на той
же странице textarea 772px, бокс ~820px. Родительский `.pp-chatflex` (row, 1620px) верен,
т.е. ужимается сама площадка композера внутри чат-колонки. Пилюля видна и ввод работает —
дефект чисто размерный; согласуется с темой бага (auto-дорожка против `fr` в шаблоне
`.chat-input-box`). Замер: `getBoundingClientRect` живой страницы, MCP eval.

## Срез 2026-11-27 — сайзинг исправлен, остаток — auto-placement

Причина сайзинг-части: `sizing_of(Fr(_)) → None` в `classify_col_tracks`
(`grid_auto_cols.rs`) — любой шаблон с fr отбраковывал новый content-сайзер целиком, сетка
падала в фолбэк `grid.rs`, где `auto_col_width = 0` при `total_fr > 0` и в intrinsic-цикле
Auto-дорожки пропускались: все auto — 0, fr забирал всё. Сделано: (1) `Sizing::Fr(f32)` —
`1fr` = `minmax(auto, fr)` (§7.2.3.2), рост-лимит flexible-дорожки не растёт айтемами
(§12.5), fr-фаза в `distribute_free_space` — §12.6/§12.7 с заморозкой; auto-дорожки при
наличии fr не растягиваются сверх max-content (Chrome на живой странице: `44px 0px 616px
40px 40px 0px 48px`); (2) indefinite auto-width inline-grid с fr (§12.8, WPT `half_fr`)
и fr-less indefinite шаблоны остались на прежних путях (гейт `has_flexible && width:none
&& InlineGrid`). Тесты: 4 новых в `grid_fr_auto` (композер-шаблон, minmax(0,auto) между
fr, заморозка §12.7, lone auto), полный lumen-layout 6144/0; пиксельная дельта run.py
пустая, dump_golden — те же 2 унаследованных несовпадения.

Живой портал после фикса (Lumen dev-release, docW 1920, vs Chrome docW 1904):
`.pp-chat-head` — `pp-head-title` 108×33 на месте (Chrome 108×33), стороны 886/886
(Chrome 878/878, разница = скроллбар) — заголовок чата ВИДЕН, геометрия совпадает.
`.chat-input-box` — дорожки с контентом вместо нулей, но размещение расходится:
Lumen `268.5px 128.5px 391px 0…` — auto-placement положил `chat-input-row` в дорожку 1
и `chat-input-bottom` в дорожку 2; Chrome `44px 0px 616px 40px 40px 0px 48px` — строка
с textarea в дорожке 3, кнопки — в 4/5/7 (стили: `html.pp-chat #screen-chat-dev
.chat-input-box{display:grid;grid-template-columns:auto auto minmax(0,1fr) auto auto
auto auto}`, сам row/bottom gc auto/auto, span-правило `:not()` их исключает — вопрос в
трактовке auto-placement/вкладов при auto-размещении). Живой Chrome в момент снятия был
в полусобранном состоянии композера (row/bottom 0×0) — честный A/B нужен на синтетике
через новый таб CDP (`/json/new`). Также осталось: computed `grid-template-columns`
отдаёт шаблон, не использованные px (Chrome отдаёт px).
