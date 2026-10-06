# BUG-1300 — Фон на холсте: явный `transparent`/`none` на `<html>` отключает распространение фона `<body>`; градиент/картинка на `<html>` рисуются только в боксе корня, а не на весь холст

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout (`crates/engine/layout/src/box_tree/entry.rs::canvas_background_color` — `html_has_bg` считает любое объявление; paint корня — градиент/картинка рисуются в бордер-боксе, не на холсте)


## Симптом

`--screenshot` 200×100:

| разметка | получено | ожидается |
|---|---|---|
| `body{background:green;margin:0}` | холст зелёный (19 955 px) | верно |
| то же + `html{background-color:transparent;background-image:none}` | зелёная только область `<body>` (3 555 px) | холст зелёный: у `html` вычисленный фон — `none`, значит берётся фон `body` (CSS Backgrounds 3 §2.11.2) |
| `html{background:red;height:50px;margin:20px}` | красный на весь холст | верно |
| `html{background:linear-gradient(red,red);height:50px;margin:20px}` | красное только в боксе корня, 7 955 px из 20 000 | весь холст: фон корня рисуется на холст, плитка — по боксу корня, повторяется |
| `html{background:linear-gradient(red,red);height:50px}` | красное 9 955 px из 20 000 | весь холст |

Первое: `canvas_background_color` (`crates/engine/layout/src/box_tree/entry.rs:801`) берёт `html`, если у него `background_color.is_some() || !background_layers.is_empty()`, а `transparent`/`none` — тоже «есть», и до `body` дело не доходит (`.to_color_opt()?` → `None`). Нужно «брать фон `body`, если у `html` вычисленный фон — `none`». Последнее: цвет корня распространяется на холст (`canvas_background_color`), слои (градиент, картинка) — нет.

## Как найдено

WPT-RUN-14 срез 6: `background-color-body-propagation-00{1,3,8,9}.html` (4), `background-attachment-margin-root-00{1,2}.html`, `background-margin-{root,transformed-root,will-change-root,iframe-root}.html` (4) — 10 reftest, все `thick`. Причина каждого отнесена по исходнику и по двум пробам выше, `margin-root`/`transformed-root` отдельно не разделялись.

## Что делать

(1) в `canvas_background_color` и в paint-проверке «у корня есть фон» смотреть на вычисленный фон (`transparent`/`none` — «нет»); (2) слои фона корня рисовать на холст с позиционированием по боксу корня (§2.11.2).

## Как проверить

`css/css-backgrounds/background-color-body-propagation-00{1,3,8,9}.html`, `background-margin-root.html`, `background-attachment-margin-root-00{1,2}.html`.
