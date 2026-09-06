# BUG-1012 — `tabIndex` getter default value конфлирует со «фокусируемо ли сейчас», а не со спековой таблицей умолчаний

**Статус:** OPEN
**Заведён:** 2026-09-06 (P2, WPT-RUN-7 срез 16 — `html/interaction`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — геттер `tabIndex`; `crates/js/src/shim/web_api_shim_tail_b.js` — `_lumen_is_focusable`, `_LUMEN_FOCUSABLE_TAGS`)
**Владелец:** P1

## Симптом

`tabindex-getter.html` (`html/interaction/focus/sequential-focus-navigation-and-the-tabindex-attribute/`),
без атрибута `tabindex` на элементе — 5 несовпадений из 30-строчного `defaultList`:

| элемент | ожидание (WPT) | факт |
|---|---|---|
| `<a>` (без `href`) | `0` | `-1` |
| `<svg><a></a></svg>` (без `href`) | `0` | `-1` |
| `<embed>` | `-1` | `0` |
| второй `<summary>` внутри одного `<details>` | `-1` | `0` |
| `<div contenteditable>` | `-1` | `0` |

## Причина

Геттер `tabIndex` (`web_api_shim_mid.js:6820`) при отсутствующем/невалидном атрибуте
`tabindex` вычисляет умолчание как `_lumen_is_focusable(nid) ? 0 : -1` — переиспользует
предикат «фокусируем ли элемент ПРЯМО СЕЙЧАС» (нужен для `focus()`/tab-order) вместо
спековой таблицы умолчаний для IDL-атрибута `tabIndex` (HTML LS §6.6.6). Эти два вопроса
дают разные ответы для одних и тех же элементов:

- `A`/`AREA`: `_lumen_is_focusable` возвращает `_lumen_has_attr(nid, 'href')`
  (`web_api_shim_tail_b.js:799`) — верно для реальной фокусируемости (`<a>` без `href` не
  ловит клик/tab), но геттер `tabIndex` по спеке обязан возвращать `0` для `a`/`area`
  **независимо от `href`**.
- `EMBED`: безусловно `1` в `_LUMEN_FOCUSABLE_TAGS` (`tail_b.js:713`) — умолчание `tabIndex`
  для browsing-context-container по спеке `-1`, а не `0`.
- `SUMMARY`: тоже безусловно `1` в той же таблице — верно только для **первого**
  `<summary>`-потомка `<details>` (он и есть summary самого `<details>`); второй и
  последующие `<summary>` элементы — обычный контент, умолчание `-1`.
- `contenteditable`: `_lumen_is_focusable` намеренно возвращает `true` для любого элемента
  с `contenteditable != 'false'` (`tail_b.js:795-796`, нужно для реального клика/фокуса на
  editing host) — но спековое умолчание IDL `tabIndex` для editing host всё равно `-1`
  (он фокусируем через отдельный «click focusability» механизм, не через tabindex focus
  flag).

Все пять — не гипотеза: код прочитан построчно и подтверждён живым выводом WPT-раннера
(`FAIL a.tabIndex should return 0 by default - assert_equals: expected 0 but got -1` и т. п.,
`html/interaction` срез 16, `--update-expected`).

## Почему это важно

`_lumen_is_focusable` используется и для настоящей фокусируемости (клик, Tab-навигация,
`focus()`), и его нельзя просто исправить «в сторону спеки» — это сломает реальное
поведение (плюс href-`<a>` в фокус-порядок не встанет, editing host потеряет клик-фокус).
Нужна вторая, отдельная функция под спековую таблицу умолчаний `tabIndex`
(HTML LS §6.6.6), а геттер должен звать её, а не `_lumen_is_focusable`.

## Воспроизведение

```
LUMEN_PROFILE=dev-release tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root html/interaction --recursive \
  --limit 1 --offset <id of tabindex-getter.html>
```
или напрямую `run_smoke.py html/interaction/focus/sequential-focus-navigation-and-the-tabindex-attribute/tabindex-getter.html`.

## Не проверялось

Полный список категорий элементов из спековой таблицы (§6.6.6) не сверялся построчно —
взяты только 5 расхождений, реально всплывших в этом прогоне; возможны и другие элементы
с тем же классом дефекта, не покрытые этим конкретным тест-файлом.
