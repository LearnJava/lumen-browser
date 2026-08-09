# BUG-732: шесть базовых DOM/CSSOM-API отсутствуют в шиме

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`)
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
`<script>`.
