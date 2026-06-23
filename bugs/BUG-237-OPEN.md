# BUG-237

**Статус:** OPEN (DEBTOR — KNOWN_DEBTORS 11.19%)
**Компонент:** layout (рендер-паритет с reference-браузером)
**Тест:** TEST-122 (diff 11.19%)

## Описание

`line-height-step` (CSS Rhythmic Sizing L1 §2): рендер Lumen расходится с Edge-эталоном.
Lumen реализует свойство и спек-корректно округляет высоту каждого line-box вверх до
кратного шагу; цветные inline-фоны заливают округлённый line-box. **Edge (Chromium) свойство
не поддерживает** — `line-height-step` был экспериментальной фичей за флагом LayoutNG и
удалён из Chromium ~2018, текущий Edge его игнорирует.

## Расследование (2026-06-23)

Свежая сборка (`--build`, dev-release), TEST-122 = 11.19%, регион x:41–627 y:32–344.

Дамп display-list (`--dump-display-list 122-line-height-step.html`) — геометрия фонов
**спек-корректна**:

```
FillRect (41,  60, 251, 24) #7ec8e3   ← natural: 24px (font 20 × 1.2), шаг не задан
FillRect (361, 72, 251, 48) #a8e6cf   ← stepped: 48px line-box (line-height-step:48px)
FillRect (361,120, 260, 48) #a8e6cf
FillRect (41, 239, 316, 60) #f9c846   ← single: 60px (line-height-step:60px)
FillRect (41, 305, 408, 40) #ffb3ba   ← child: 40px (унаследован от .parent)
```

Все четыре блока округлены вверх до своего шага (48/60/40), наследование `.parent`→`.child`
работает — это точное поведение CSS Rhythmic Sizing L1 §2.

**Edge-эталон (`122-line-height-step-edge.png`):** обе колонки идентичны — зелёные
stepped-полосы той же высоты 24px, что и синие natural, строки с шагом 24px. Edge
**не применяет** `line-height-step` вовсе (рисует natural fallback). То же для single/child
блоков.

Расхождение распадается на:
1. **`line-height-step` не поддержан в Edge** (доминирует): Lumen рисует 48/60/40px
   line-box, Edge — 24px natural. Совпасть можно лишь отключив свойство в движке —
   это сделало бы Lumen *менее* спек-корректным.
2. **font-parity переноса** (rule 3): Inter шире шрифта Edge → natural-колонка
   переносится в 3 строки вместо 2; накапливается по всем блокам.

## Почему не фикс

Тот же класс, что [BUG-126](BUG-126-OPEN.md)/TEST-77 (`inset-area` deprecated) и
[BUG-199](BUG-199-OPEN.md)/TEST-71 (`@starting-style`): **Lumen спек-корректнее
reference-браузера**. Привести diff к 0.5% можно только удалив рабочую реализацию
`line-height-step` — понижение возможностей движка ради совпадения с браузером, который
фичу выпилил. Это запрещено правилом «фиксить движок, а не понижать планку».

**Решение:** KNOWN_DEBTORS с baseline 11.19% (±2% gdigrab-допуск). При появлении в Edge
поддержки `line-height-step` (маловероятно) или смене эталона — пересмотреть.
