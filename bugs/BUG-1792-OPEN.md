# BUG-1792 — lenta.ru: `Uncaught TypeError: e.children is not iterable` в коде плеера/виджетов

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** js/dom — `HTMLCollection`/`children` итерируемость на каком-то интерфейсе

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153, `--maximized`, `LUMEN_NO_ADBLOCK=1`), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://lenta.ru` — стабильное JS-исключение `Uncaught TypeError: e.children is not iterable` в обоих прогонах (не flaky).
- Бросает минимизированный код плеера/виджетов lenta; точный интерфейс не локализован.

## Гипотезы

- `children` (HTMLCollection) без `Symbol.iterator` на конкретном элементе — например SVG-элемент, MathML или элемент внутри shadow DOM, где коллекция собирается другим путём.
- Либо итерируется не `children`, а результат `.children` на `Document`/`DocumentFragment` в контексте, где у нас возвращается другой тип.
- Проверить spread `[...el.children]`, `for..of`, `Array.from` по children на: HTML-элементах, SVG-элементах (`SVGElement`/`SVGSVGElement`), внутри shadow root, на `template.content`.

## Как локализовать

- Живой прогон `lumen --maximized https://lenta.ru` (`LUMEN_NO_ADBLOCK=1`), health-log/console — вытащить стек исключения и имя интерфейса `e`.
- Сравнить с Chrome: тот же сайт без исключений.
- Разница с рабочими сайтами — найти, какой DOM-API вернул не-итерируемое.

## Проверка

- compat_crawl на lenta.ru: exception-чек зелёный.
- Регрессионный тест в `crates/js`/`crates/engine/dom` на итерируемость `children` по всем интерфейсам, где она по спеке есть (ParentNode/NonElementParentNode миксины, SVG).

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
