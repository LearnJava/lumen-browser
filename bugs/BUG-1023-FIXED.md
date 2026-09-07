# BUG-1023 — `:heading()`/`:heading(N)` селектор и `headingOffset`/`headingReset` не реализованы

**Статус:** FIXED 2026-09-07 (P3)
**Заведён:** 2026-09-06 (P2, WPT-RUN-7 срез 19 — `html/semantics/sections`)
**Область:** css-parser (селекторы — нет `:heading`/`:heading(N)`) + dom (нет IDL-отражения
`headingOffset`/`headingReset` контент-атрибутов на `HTMLElement`)

## Симптом

`headingoffset-and-headingreset.html` и `headingoffset-mutations.html` — оба файла
целиком FAIL (0/100 сабтестов пройдено на категорию из двух файлов), второй ещё и
`expected: ERROR` на уровне харнесса целиком:

```
FAIL headingoffset (set via attribute) should change the level a heading matches against
FAIL case 1: heading level for <h1 data-expected-offset="2">...</h1> should match based on
  expected document structure
```

## Причина

Оба теста реализуют новый черновик WHATWG (`whatwg/html#11086`, ссылка в тесте) —
вычисляемый «уровень заголовка» (`heading level`) с учётом вложенных контейнеров
`headingoffset="N"`/`headingreset`, доступный через:
- CSS-псевдокласс `:heading` / `:heading(N)` (matches любой `<h1>`–`<h6>` на своём
  эффективном уровне после применения offset/reset цепочки предков);
- IDL-атрибуты `HTMLElement.headingOffset` (число) / `HTMLElement.headingReset`
  (boolean), отражающие одноимённые контент-атрибуты.

Ни то, ни другое не реализовано: `:heading()` не существует как псевдокласс в
css-parser (значит `el.matches(":heading(1)")` кидает `SyntaxError` вместо булева
результата — источник `ERROR` на харнесс), а `headingOffset`/`headingReset` на
`HTMLElement` не определены вовсе (`modal.headingReset` читает `undefined`, не
булево — источник провала остальных ассертов).

## Почему это важно

Фича экспериментальная и очень новая (черновик 2024/2025, ссылка на PR, не RFC) —
не блокер реального контента, но полностью нулевое покрытие (0/100) означает, что
`--check` на этой категории не отловит вообще никакую регрессию до тех пор, пока
фича не появится хотя бы частично.

## Возможный путь фикса

Два независимых куска: (1) псевдокласс `:heading`/`:heading(N)` в
`css-parser`/селекторный матчер (эффективный уровень = базовый уровень тега h1-h6
минус 1 плюс сумма `headingoffset` всех предков-контейнеров до ближайшего
`headingreset`/модального `<dialog>`, клампится в [0, 9]); (2) IDL-отражение
`headingOffset`/`headingReset` в JS-шиме аналогично другим числовым/булевым
content-attribute reflection-полям. Оценка объёма не делалась — не смотрелось,
есть ли уже общий helper для «эффективного уровня заголовка» где-то в a11y-дереве
(`crates/engine/a11y`), которым можно было бы переиспользовать вычисление.

## Воспроизведение (до фикса)

```
tests/wpt/.venv/bin/python3 tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/semantics/sections \
  --recursive --update-expected
```

## Не проверялось

Остальные `html/semantics/*` под-пути на предмет того же класса (другие новые
селекторы/IDL-отражения) — не искалось системно, найдено только на этих двух
файлах.

## Фикс (2026-09-07, P3)

Две независимые части, как в §Возможный путь фикса:

1. **CSS-псевдокласс.** `PseudoClass::Heading(Option<i32>)` в
   `crates/engine/css-parser/src/parser/selectors.rs` — `None` для bare
   `:heading` (парсится веткой простых псевдоклассов), `Some(n)` для
   `:heading(n)` (функциональная форма — не an+b формула и без `of`-clause,
   поэтому это просто raw-token collect до `)` + `str::parse::<i32>`, ближе к
   `:state(<ident>)`, чем к `:nth-child`). Невалидный/пустой аргумент падает в
   `Unsupported` как у всех остальных функциональных псевдоклассов.
   Сериализация (`pc_to_css_str`) и specificity (уже покрыта общим
   `_ => spec.b += 1` в `accumulate_specificity`) не потребовали отдельного
   кода для specificity.

2. **Матчер.** `matches_heading`/`effective_heading_level` в
   `crates/engine/layout/src/style/matching/forms.rs`, по образцу
   `matches_lang`/`element_lang` (walk-up по `Node.parent`). Алгоритм
   (реконструирован по самой WPT-фикстуре, не по тексту PR — черновик
   формулу явно не даёт):
   - базовый уровень тега (`h1`=1 … `h6`=6), `None` для остального;
   - сумма `headingoffset` по цепочке **предок-или-сам-элемент** (сверху
     вниз), где каждое отдельное значение сперва клампится в `>= 0`
     (отрицательный `headingoffset` игнорируется, а не вычитается — WPT
     явно проверяет «Negative headingoffsets are clamped to `0`» для
     значения атрибута, не для итоговой суммы);
   - на ближайшем `headingreset` предке-или-себе накопленная сумма
     обнуляется, но собственный `headingoffset` ЭТОГО ЖЕ узла всё равно
     прибавляется после обнуления (WPT: «Resetting applies after
     headingOffset» — контейнер с ОБОИМИ атрибутами разом даёт
     `база + свой_offset`, не `база`);
   - итог клампится в `[1, 9]`.

   Никакого специального кода для shadow DOM/`<slot>` не понадобилось: обычный
   `Node.parent` уже даёт слотированному контенту его исходную (light-DOM)
   цепочку предков, а не цепочку `<slot>` — ровно то, что фикстура проверяет
   явно («Ensure the slot's headingoffset does not affect slotted content»).
   Раннее предположение в §Возможный путь фикса о роли модального `<dialog>`
   не подтвердилось: фикстура прямо проверяет, что `showModal()` НЕ включает
   implicit `headingreset` (`assert_false(modal.headingReset, "modal dialogs
   return headingReset false by default")`) — `<dialog>` для этого алгоритма
   ничем не отличается от любого другого контейнера.

3. **IDL-отражение.** `headingOffset`/`headingReset` добавлены в табличную
   `_lumen_install_reflection(HTMLElement.prototype, […])`
   (`crates/js/src/shim/web_api_shim_tail_b.js`) — `ulong`/`bool`, как у всех
   прочих numeric/boolean reflection-полей. `ulong` (не `long`) — чтобы
   `el.headingOffset = -1` тоже клампилось к 0 через IDL-сеттер, а не только
   через прямой `setAttribute`.

   `crates/engine/a11y` (упомянутый в §Возможный путь фикса как потенциальный
   источник переиспользуемого «уровня заголовка») оказался нерелевантен:
   его `aria-level` — отдельная, независимая от `:heading` концепция
   (WPT-фикстура явно показывает `<h1 headingoffset="9" aria-level="3">` с
   ожидаемым `:heading`-уровнем 9, то есть `aria-level` НЕ участвует в этом
   вычислении вовсе).

**Тесты:** 4 парсинг-теста в `crates/engine/css-parser/src/parser/tests/selectors.rs`
(`pseudo_heading_*`); 10 матчинг-тестов в новом
`crates/engine/layout/src/tests/heading_level_pseudo.rs`, воспроизводящих
конкретные примеры из самой WPT-фикстуры (offset-накопление, reset,
clamping вверх/вниз, «reset applies after own offset»). `cargo test -p
lumen-css-parser --lib` (443/443) и `-p lumen-layout --lib` (3929/3929, 1
ignored) зелёные, регрессий нет.

**WPT (`tests/wpt/run_report.py --root html/semantics/sections
--update-expected`, дерево `dev-release`):**
- `headingoffset-and-headingreset.html`: 0/61 → **61/61 сабтестов, Test OK**
  (было 0/100 на всю категорию из двух файлов).
- `headingoffset-mutations.html`: 0/100 → 65/100 сабтестов; харнесс остаётся
  `ERROR` — 3 FAIL + 32 NOTRUN упираются в `Cannot read properties of null
  (reading 'matches')` внутри shadow DOM/`<slot>` мутационных сценариев, не
  в `:heading`. Корень — уже заведённые дыры: `slot.assignedNodes()`/
  `slotchange` не работают вовсе ([BUG-876](BUG-876-OPEN.md), ДОРАБОТКА →
  GAP-SLOT) и `element.shadowRoot` строит новую обёртку на каждое чтение
  ([BUG-877](BUG-877-OPEN.md)). Новых багов не заводилось — это тот же
  класс дефектов, что уже в очереди, не следствие фикса этого бага.
  `tests/wpt/metadata/html/semantics/sections/headingoffset-mutations.html.ini`
  обновлён `--update-expected`, чтобы будущий `--check` на этой категории
  ловил регрессии, а не переоткрывал уже известную дыру.
