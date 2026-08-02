# BUG-466: margins do not collapse through an empty block

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (block formatting context, margin collapsing, CSS2.1 §8.3.1)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`,
`css/CSS2/normal-flow/margin-collapse-through-for-various-height-values.tentative.html`
(72/72 сабтеста FAIL)

## Симптом

```
FAIL <case> - assert_equals: margins should collapse through expected 50 but got 0
```

Все 72 сабтеста (варианты по `height`: auto/0/значения с `calc()`/
`min-height:stretch` и т.п.) проваливаются одинаково: когда пустой блок не
создаёт собственный BFC и не имеет padding/border/содержимого, отделяющего его
margin-top от margin-bottom, оба margin'а должны схлопнуться друг с другом и
"пройти сквозь" блок наружу (margin collapsing through), так что margin
родителя выше и margin следующего элемента ниже сливаются в один = `max(...)`
двух margin'ов (в тесте — 50px). Lumen вместо этого измеряет 0 — margin
пустого блока не распространяется наружу вовсе, схлопывание "через" блок не
реализовано.

Формализует находку, отмеченную ещё во время точечной проверки WPT-RUN-1
(`docs/wpt-status.md`, строка `css`, проба ~11:30): "несколько кейсов
`min-height:stretch`/`calc()` margin-collapse-through проваливаются" — тогда
без отдельного BUG-NNN, теперь заведено формально.

## .ini

`tests/wpt/metadata/css/CSS2/normal-flow/margin-collapse-through-for-various-height-values.tentative.html.ini`
— все 72 сабтеста `expected: FAIL`.
