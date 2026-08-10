# BUG-732: шесть базовых DOM/CSSOM-API отсутствуют в шиме

**Статус:** FIXED 2026-08-10 (пять точек из шести; шестая → [BUG-746](BUG-746-OPEN.md))
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`) + layout/shell (снимок custom properties)
**Найден:** P3 при пересъёмке [BUG-725](BUG-725-FIXED.md), 2026-08-09

## Симптом

Проверено пробой на тривиальной локальной странице
(`<h1 id="h" class="k" data-x="1">hi</h1>` + `<style>:root{--probe:7px}</style>`,
живое окно через `--mcp-live-port`) — все шесть воспроизводятся вне зависимости
от сайта:

| API | `typeof` / значение | Ожидание |
|---|---|---|
| `Node.prototype.contains` | `undefined` | функция |
| `Node.prototype.compareDocumentPosition` | `undefined` | функция |
| `Element.prototype.attributes` | `undefined` | `NamedNodeMap` |
| `document.styleSheets` | `undefined` | `StyleSheetList` |
| `document.images` | `undefined` | `HTMLCollection` |
| `getComputedStyle(el).getPropertyValue('--probe')` | `""` | `"7px"` |

Последний — не отсутствие метода: обычные свойства тот же объект отдаёт верно
(`getPropertyValue('color')` → `rgb(0, 0, 0)`), не работают именно custom
properties. Движок значение знает — каскад их применяет (см.
[BUG-731](BUG-731-FIXED.md)), наружу оно не выведено.

`compareDocumentPosition` подтверждён и на живой `tbank.ru` — в консоли
`TypeError: n.compareDocumentPosition is not a function` (сторонний скрипт).

## Почему это одна заявка, а не шесть

Все шесть — отсутствующие точки одного шима, каждая закрывается локальным
добавлением в `WEB_API_SHIM` (движковые данные для всех шести уже есть). Заводить
их отдельными номерами смысла нет; если при реализации какая-то потребует
движковой работы (вероятнее всего `styleSheets` — нужен доступ к разобранному
`Stylesheet`) — выделить её в свой номер.

`contains` и `compareDocumentPosition` — самые ходовые: на них построены
проверки «этот узел внутри того» в React/аналитике, а провал даёт `TypeError`
посреди чужого кода, который дальше не выполняется вовсе.

## Ловушка при проверке

Страница без единого `<script>` не поднимает JS-контекст (`eval` отвечает
`JS context not available`) — в пробную страницу нужно класть хотя бы пустой
`<script>`. **Уточнение по факту проверки: пустого `<script></script>` мало,
контекст поднимается только с непустым телом** (`<script>window.x=1;</script>`).

## Исправление (2026-08-10)

Пять точек из шести. Шестая (`document.styleSheets`) выделена в
[BUG-746](BUG-746-OPEN.md) ровно по критерию, записанному в разделе выше:
готовых движковых данных для неё в JS-рантайме нет вообще — разобранный
`Stylesheet` живёт в шелле, натива на чтение не существует
(`grep -rn "styleSheets\|CSSStyleSheet\|cssRules" --include=*.rs crates/` даёт
единственное совпадение, и то комментарий), так что это не «локальное
добавление в шим», а плюмбинг публикации плюс CSSOM-объекты.

**`Node.contains` / `Node.compareDocumentPosition`** (`dom.rs`, рядом с
`Node.prototype.hasChildNodes`) считают по id узлов арены, а не по
идентичности JS-обёрток. Это не оптимизация, а условие правильности: живой
`document` — объектный литерал без `__nid__`, а `documentElement.parentNode`
отдаёт *обёртку корневого узла*, а не сам `document`, поэтому обход вверх по
`parentNode` со сравнением по `===` дал бы `document.contains(el) === false`
для каждого элемента страницы. Тот же литерал не подключён к
`Node.prototype`, поэтому у него собственные копии обоих методов (как у
`hasChildNodes`, BUG-327). Вместе с методом заведены константы
`Node.DOCUMENT_POSITION_*`: типовой вызов — `pos & Node.DOCUMENT_POSITION_…`,
и без имён маска молча вырождается в `0`, то есть метод без констант
бесполезен.

**`Element.attributes`** — живой `NamedNodeMap` на Proxy (та же схема, что у
`_lumen_make_nid_collection`) поверх уже существовавших
`_lumen_get_attr_names`/`_lumen_get_attr`, с `Attr`-узлами, которые читают и
пишут напрямую в элемент. Заодно — `getAttributeNode`/`setAttributeNode`/
`removeAttributeNode`, потому что возвращать `Attr` больше неоткуда.

**`document.images`** — живая `HTMLCollection` (а не статический массив, как у
`getElementsByTagName`): её читают повторно долгоживущие прелоадеры/ленивая
загрузка, ожидая увидеть вставленные позже картинки.

**`getComputedStyle().getPropertyValue('--x')`** потребовал движковой части:
custom properties публикуются **отдельным** снимком за `Arc`
(`lumen_layout::collect_custom_properties` → `V8JsRuntime::update_custom_properties`,
натив `_lumen_get_custom_property`), а не подмешиваются в поэлементную карту
стандартных свойств. Причина — стоимость: они наследуются, поэтому один набор,
объявленный в `:root`, это одна copy-on-write аллокация на весь документ
(`CustomProps`), а вложение их в карту стандартных свойств переписало бы каждую
переменную на каждый узел и умножило снимок, который шелл пересобирает после
**каждого** relayout, на число объявленных переменных — на странице
дизайн-системы это тот же снимок ещё несколько раз. Значения в снимке —
computed: `var()`/`env()` подставлены (`expand_vars_and_env`), неразрешимая
ссылка отдаётся пустой строкой (guaranteed-invalid, CSS Variables L1 §3.3).
Каждая различная карта резолвится ровно один раз, дальше раздаётся тот же
`Arc` (тест `custom_property_snapshot_shares_one_allocation_per_distinct_map`).

Проверка: `.tmp/b732_probe.py` (живое окно, локальная страница) — все пять
точек и краевые случаи (`var()`-цепочка, битая ссылка, живость коллекций и
карты атрибутов, запись через `Attr.value`, текстовые узлы в
`compareDocumentPosition`). Юнит-тесты: `dom.rs::v8_bug732_node_and_collections`
(7), `dom.rs::v8_computedstyle::get_computed_style_custom_property*` (2),
`lumen-layout::custom_property_snapshot_*` (2).
