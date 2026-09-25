# BUG-1127 — Относительные `url()` во внешнем CSS разрешаются от URL документа, а не таблицы стилей

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/subresources.rs:193` `load_font_faces` игнорирует `_base`; `crates/shell/src/page_load.rs:2175` `base.resolve_str(&pf.url)` — база документа; фон — `subresources.rs:~101`; `stylesheets.rs` склеивает тексты листов без их базы)

## Симптом

CSS Values §4.3 (и CSS Syntax: база `url()` — URL таблицы стилей). В Lumen база — документ:

- apple: `ac-globalfooter.built.css` с `url("../assets/ac-footer/legacy/appleicons_text.woff")` →
  Lumen `GET https://www.apple.com/assets/…` → 404; правильный `/ac/globalfooter/8/en_US/assets/…`
  отдаёт 200. Нет иконочных шрифтов футера.
- tumblr: `@font-face` из `main-dbd400ec.css` разрешается от `www.tumblr.com` → 301 на
  `assets.tumblr.com/fonts/…` → 404.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g5/fontbase.html`:

```html
<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="css/s.css"></head>
<body><div id="f">Hello font</div><div id="b"></div><script>window.z=1</script></body></html>
```

`g5/css/s.css`:

```css
@font-face { font-family: "G5Font"; src: url("fonts/f.ttf") format("truetype"); }
#f { font-family: "G5Font", monospace; }
#b { width: 40px; height: 40px; background-image: url("bg.svg"); }
```

**Результат:** Lumen: `GET /fonts/f.ttf` → 404 («@font-face «G5Font»: не загружен fonts/f.ttf: network error: HTTP 404»), `GET /bg.svg` → 404. Chrome: `/css/fonts/f.ttf`, `/css/bg.svg`, `document.fonts` — `G5Font:loaded`.

## Что сделать

Резолвить `url()` каждой таблицы от её собственного URL (для `@import` — от URL
импортированного листа, для `<style>` — от базы документа) в момент разбора, до склейки листов.
Критерий: репро запрашивает `/css/…`; шрифты футера apple загружаются.

## Ещё сайт: khanacademy (2026-09-25, P6, перемер при BUG-1120)

`https://cdn.kastatic.org/khanacademy/khanacademy.*.css` объявляет `@font-face` с
`url(fonts/19b341e83898cab0-NotoSans-Regular.woff2)`. Lumen запрашивает
`https://www.khanacademy.org/fonts/…` → 403, ни один шрифт страницы не грузится; верный адрес
`https://cdn.kastatic.org/khanacademy/fonts/…` отдаёт 200.
