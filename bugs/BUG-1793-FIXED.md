# BUG-1793 — accName ссылок с svg/img-иконками пуст: accessible name не берёт alt/title из содержимого

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** a11y — вычисление accessible name (accname), `crates` a11y-дерево
**Тип:** дефект вычисления (не пробел возможностей)

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153, `--maximized`, `LUMEN_NO_ADBLOCK=1`), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://www.rust-lang.org` — a11y-покрытие 52/57: «лишние» link-узлы без имени — логотипы-ссылки YouTube, Rust, Mastodon, GitHub, Bluesky.
- `https://docs.rs` — a11y 19/24: ссылки на крейты с иконками — нет accessible name.
- У эталона (Chrome) те же узлы имеют имя — accName по спецификации (accname 1.1/1.2) для `<a>` без собственного текста должен собираться из содержимого, включая `alt`/`title`/`aria-label` вложенных `img`/`svg`.

## Причина

`names::compute_name` брал «текст содержимого» через `collect_text_content` — обход, который
конкатенирует **только** `Text`-узлы (innerText-подобный). У ссылки-иконки текстового узла нет
вовсе, поэтому имя выходило пустым:

```html
<a rel="me" href="https://social.rust-lang.org/@rust"
   ><img src="/static/images/mastodon.svg" alt="Mastodon" title="Mastodon"></a>
```

ACCNAME-1.2 §4.3.2 step 2F описывает name-from-content как сумму **текстовых альтернатив**
потомков, а не только их текста. Спец-случай был захардкожен лишь для `<button>` с единственным
`<img>`-ребёнком, поэтому кнопки-иконки имя получали, а ссылки — нет.

## Что сделано

`crates/engine/a11y/src/names.rs`: `collect_name_from_content` / `collect_name_recursive` —
рекурсивный сбор имени по содержимому вместо простой конкатенации текста:

- `Text`-узлы — как раньше;
- скрытые поддеревья (`aria-hidden="true"`, `hidden`) не дают ничего (§4.3.2 step 2A);
- собственный `aria-label` потомка заменяет его содержимое (step 2C/2D/2E);
- `alt` у `<img>`/`<area>`/`<input type=image>`; `alt=""` — декоративен и в имя не входит;
- `<title>`-потомок у `<svg>` (SVG-AAM), приоритетнее видимого `<text>` внутри графики;
- `title`-атрибут потомка — последнее средство, только если ни текста, ни альтернативы нет (step 2I);
- глубина рекурсии ограничена `MAX_NAME_DEPTH = 64`.

Функция заменила `collect_text_content` во всех путях name-from-content: `<a href>`, заголовки
`<h1>`–`<h6>`, `<summary>`, `<button>` и общий фолбэк шага 4. `collect_text_content` остался для
описания, `aria-labelledby`-ссылок и `<label>`-связей — там поведение не менялось.

## Проверка

- `cargo test -p lumen-a11y --test all` — 155/155 (5 новых: `link_name_from_nested_img_alt`,
  `link_name_from_nested_svg_title_child`, `link_name_from_nested_svg_aria_label`,
  `link_name_mixes_icon_alt_and_text`, `link_decorative_img_stays_unnamed`).
- `cargo clippy -p lumen-a11y --all-targets -- -D warnings` — чисто.
- Живой compat-crawl (`--exe target/dev-release/lumen.exe`, `LUMEN_NO_ADBLOCK=1`):
  - **rust-lang.org — a11y PASS** (было 52/57): 5/5 проверок, CPR 100 %, `.tmp/compat-1793/`.
  - **docs.rs — 19/24 → 22/24** (порог 95 %, поэтому сайт в отчёте остаётся `DIFF[a11y]`).
    Остаток — не accname: все несовпавшие узлы -- это ссылки блока «Recent Releases», в имя
    которых Chrome подмешивает относительный возраст выпуска
    (`… open-arrow-access-storage-0.2.0 storage adapters for open arrow access 3 seconds`,
    `… bpfix-0.1.11 userspace ebpf verifier failure diagnostic cli one minute ago`). Текст
    тикает между плечами (Lumen снимает дерево на несколько секунд позже), а набор «свежих»
    крейтов на главной меняется каждую минуту: повторный прогон дал 19/24 с другим набором
    крейтов (`.tmp/compat-1793b/`, `.tmp/compat-1793c/`). Имя у этих ссылок собирается —
    расходятся только секунды внутри него.
- `python graphic_tests/dump_golden.py --build` (профиль `dev-release`) — 12/12 дампов
  совпадают с эталоном; пикселей правка не двигает.

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
- ya.ru a11y 36/63 — частично тот же дефект (кнопки-иконки без имени), но там ещё и BUG-1791 (battery,
  уже закрыт).
