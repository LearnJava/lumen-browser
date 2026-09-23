# BUG-1047: `position:absolute` + `width:auto` + только `right` (без `left`) растягивается на всю ширину containing block вместо shrink-to-fit

**Статус:** FIXED 2026-09-23 (P3)
**Найден:** P6, живой сайт (`bankruptcy-platform`, внешний Keycloak-стенд), 2026-09-11

## Симптом

Иконка-кнопка «показать/скрыть пароль» внутри `<input type="password">`
перехватывает клики и ввод текста по всей ширине поля, а не только в
области самой иконки — `click`/`type` через MCP на сам `#password` либо
попадают в кнопку («Element click intercepted: point (x, y) hits
`<button>` … instead of the target element»), либо, если клик попадает по
чистому `{point:…}` (в обход BUG-1044 hit-check), фокус всё равно уходит
на `<button>`, и последующий `type` падает с `Element is not a mutable
text field` — сам `<input>` при этом никогда не получает фокус штатным
кликом.

## Причина

Разметка — типичный паттерн «иконка внутри инпута»:

```html
<div class="password-wrapper" style="position:relative; width:420px">
  <input id="password" type="password" class="field-input">
  <button class="eye-btn" style="position:absolute; right:12px; /* left не задан */">
    <svg>…</svg>
  </button>
</div>
```

Замер через `getComputedStyle`/`getBoundingClientRect` в живом окне:

```
wrapper: {rect: [1153.2, 257.18, 420, 48]}
button:  {rect: [1141.2, 281.18, 420, 21], computedWidth: "auto",
          computedPosition: "absolute", computedInsetRight: "12px"}
```

У кнопки `position:absolute; width:auto; right:12px`, **`left` не
указан**. По CSS 2.1 §10.3.7 (resolution algorithm для абсолютно
позиционированных блоков, «Rule 1»–«Rule 3») это ровно тот случай, где
`left` — `auto`, и при `width:auto` кнопка должна получить width по
shrink-to-fit (по содержимому — SVG-иконка, реалистично ~16–20px), а не
растягиваться до ширины containing block. Lumen же выдаёт кнопке
`width: 420px` — буквально ширину `.password-wrapper`, как будто заданы
ОБА инсета (`left:0; right:12px`), из-за чего блок обязан растянуться.

Итоговый layout-бокс кнопки перекрывает практически весь `<input>` по
ширине (кнопка `x=1141` при wrapper `x=1153` — даже чуть шире и левее
самого инпута), поэтому hit-test в любой точке поля находит `<button>`
раньше `<input>`.

Не локализовано глубже getComputedStyle/getBoundingClientRect — сам код
резолва auto-width для `position:absolute` с одним заданным инсетом не
найден (`crates/engine/layout/src/box_tree/*`, кандидаты `bfc.rs`,
`multicol_abspos.rs` — ни один не содержит явной реализации CSS 2.1
§10.3.7 «Rule 1-3» resolution, возможно свёрнуто в общий intrinsic-width
путь `box_tree/intrinsic.rs`).

## Как воспроизвести

Живой стенд `http://my-stand.local/` (bankruptcy-platform, Keycloak login
form), поле `#password`. Или минимальный кейс — любая страница с:

```html
<div style="position:relative">
  <input>
  <button style="position:absolute; right:12px">×</button>
</div>
```

и сравнить `button.getBoundingClientRect().width` с шириной обёртки —
в Chrome/Edge будет узкой (по содержимому), в Lumen равна ширине обёртки.

## Влияние

Блокирует MCP-автоматизацию (`click`/`type`) на любом поле с подобным UI
паттерном («показать пароль», «очистить поле», «поиск с иконкой» и т.п.)
— крайне распространённая вёрстка. Не блокирует обычного пользователя
мышью (клик по видимой иконке всё равно физически в её пределах, хотя
теперь и остальная площадь поля тоже кликабельна как кнопка — с реальной
мышью это может давать неожиданный «щелчок мимо» при клике рядом с текстом
пароля).

## Побочная находка на том же стенде

Изначально принят за баг зависший после логина дашборд
(`TypeError: Cannot read properties of null (reading 'data')` в
`react-hot-toast`/`goober`, инжекция стилей через `styleEl.firstChild.data`)
— на момент обнаружения тестовый `lumen.exe` был собран **до** правки
BUG-982 (`innerHTML` теперь верно сохраняет whitespace-only фрагмент),
пересборка `dev-release` полностью убрала симптом. Отдельного бага не
заводится — ложная тревога от устаревшего бинарника, а не факт неполадки.

## Корень

Найден: `crates/engine/layout/src/box_tree/multicol_abspos.rs::abs_box_shrinks_to_fit`
— функция, введённая BUG-745 для ровно этого класса задач (абсолютный
не-replaced бокс с одной заданной инсетой должен shrink-to-fit, а не
растягиваться на containing block), исключала из shrink-to-fit **весь**
`BoxKind::FormControl` целиком, включая `<button>`/`<select>`. Комментарий
при её исключении был верным на момент BUG-745: тогда form control
считался чистым replaced-элементом без собственного контента, измеримого
через `max_content_outer_width`/`min_content_outer_width` (те не видят
контент формы — он не лежит в `b.children` в общем виде).

BUG-926 (позже) добавил `intrinsic.rs::form_control_fit_content_width` —
именно для `Button`/`Select` их used-width теперь считается по
отрендеренному содержимому (`<button>` рендерит дочерние боксы — иконку/
текст — как обычный shrink-to-fit по поддереву). Но `abs_box_shrinks_to_fit`
не обновили вслед за этим: она по-прежнему трактовала `<button>` как
«нет измеримого контента» и падала в ветку `else { cb.width }` —
буквально ширина containing block, что и давало симптом.

## Фикс

`abs_box_shrinks_to_fit` теперь допускает в shrink-to-fit `FormControlKind::Button`
и `FormControlKind::Select` (остальные виды form control — checkbox,
radio, текстовые поля, range и т.п. — остаются на старом
replaced-элементном пути: у них нет отрендеренного лейбла для измерения).

В ветке резолва ширины (`lay_out_abs_children`) `max_content`/`min_content`
для абс-позиционированного child теперь сначала пробуют
`form_control_fit_content_width`: если она вернула `Some`, оба предела
(`max_c`/`min_c`) берутся из неё (контент кнопки не переносится по
строкам, поэтому max-content == min-content), иначе — прежний путь через
`max_content_outer_width`/`min_content_outer_width`.

## Проверка

Новый юнит-тест `box_tree::tests::flow_modes::bug1047_abs_button_auto_width_shrinks_to_fit`
— абс-позиционированный `<button>` с `right` (без `left`) внутри 400px
containing block и SVG-подобной 20px иконкой внутри: ширина кнопки теперь
~22px (иконка + UA border/padding кнопки) вместо 400px.

`cargo test -p lumen-layout --lib` — 4000 passed, 0 failed.
`cargo clippy -p lumen-layout --all-targets -- -D warnings` и
`cargo clippy --workspace --all-targets -- -D warnings` — чисто.
`LUMEN_PROFILE=dev-release python graphic_tests/dump_golden.py` — все 12
дампов совпадают с эталоном (display-list neutrality на существующем
корпусе; ни один golden-файл не использует этот UI-паттерн — специально
покрыто новым юнит-тестом выше). Полный `graphic_tests/run.py` не удалось
прогнать в этой среде — TEST-00 калибровка (`gdigrab`-захват magenta-маркера)
падает независимо от содержимого правки (нет реального фокусируемого
рабочего стола в этой сессии), см. `docs/graphic-tests.md` о требовании
живого фокусированного окна.

`bash scripts/scoped-test.sh` — 4150 passed, 3 failed
(`dom::tests::v8_webworker::worker_add_event_listener_fires_on_pump`,
`worker_data_url_base64_script`, `worker_top_level_exception_fires_parent_onerror`);
все три проверены изолированно (`cargo test -p lumen-js -p lumen-shell
-p lumen-driver --lib v8_webworker -- --test-threads=1`) — 51/51 passed,
т.е. падения scoped-test — гонка при параллельном запуске воркеров, не
регрессия этой правки (модуль `web_api_shim_head.js`/`worker.rs` этим
коммитом не тронут).
