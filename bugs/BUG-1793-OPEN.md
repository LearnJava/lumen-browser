# BUG-1793 — accName ссылок с svg/img-иконками пуст: accessible name не берёт alt/title из содержимого

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** a11y — вычисление accessible name (accname), `crates` a11y-дерево

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153, `--maximized`, `LUMEN_NO_ADBLOCK=1`), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://www.rust-lang.org` — a11y-покрытие 52/57: «лишние» link-узлы без имени — логотипы-ссылки YouTube, Rust, Mastodon, GitHub, Bluesky.
- `https://docs.rs` — a11y 19/24: ссылки на крейты с иконками — нет accessible name.
- У эталона (Chrome) те же узлы имеют имя — accName по спецификации (accname 1.1/1.2) для `<a>` без собственного текста должен собираться из содержимого, включая `alt`/`title`/`aria-label` вложенных `img`/`svg`.

## Что сделать

- Найти вычисление accessible name в a11y-коде (искать `accname`/`accessible_name`/`a11y` в crates).
- Для ссылки/кнопки без текстового содержимого имя должно включать:
  - `alt` у `<img>` (даже при `alt=""` — декоративность учитывается по правилам accname);
  - `title`/`aria-label` у `<svg>` и его `<title>`-потомка;
  - `aria-label`/`title` самого элемента.
- Проверить на rust-lang.org и docs.rs — узлы должны получить имена.

## Проверка

- compat_crawl: a11y-чек rust-lang.org и docs.rs зеленеет.
- Точечный тест: `<a href><svg><title>X</title></svg></a>` и `<a href><img alt="Y"></a>` дают name в a11y-дереве.

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
- Похожие места: ya.ru a11y 36/63 (частично тот же дефект — кнопки «включить звук», «лайк» могут быть иконками без имени, но там ещё и BUG-1791 battery).
