# BUG-1827 — `cargo clippy -p lumen-js --all-targets` (без `v8-backend`) красный на чистом main: 85 ошибок never-used в таргете `lib`

**Статус:** OPEN
**Заведён:** 2026-11-18 (P6, попутно фиксу [BUG-1820](BUG-1820-FIXED.md))
**Область:** js (`crates/js/src` — хелперы, потребляемые только v8-глейм)

## Симптом

`cargo clippy -p lumen-js --all-targets -- -D warnings` на чистом main `e0f8aff95`
без фичи падает уже в таргете `lib` — 85 ошибок `never used` / `unused import`
(измерено stash-сравнением 2026-11-18; множества ошибок с веткой BUG-1820
идентичны по этим строкам — дефект не от правки BUG-1820):

- `compression.rs` — весь инкрементальный inflate-API (`cs_new`/`cs_push`/…):
  потребители — только `into_v8_fn*`-глейм `dom.rs:721–734`;
- `noise_key.rs` / `canvas2d.rs` — `noise_key`, `BUNDLED_FONT`, текстовые
  метрики (`parse_canvas_font_size`, `measure_text_width`, `baseline_offset_px`);
- `dom.rs` — константы шимов `EVENT_SHIM`, `WEB_API_SHIM_*`, `URL_SHIM` и
  нативы `url_*`/`origin_*`/`timezone_script`;
- `worker.rs`, `push_api.rs`, `paint_worklet.rs`, `offscreen_canvas`, `lib.rs`
  (`deterministic_patch_script`) — «never used» по всей длине цепочек, чьи
  единственные потребители стоят под `#[cfg(feature = "v8-backend")]`.

На `e0f8aff95` таргет `lib test` после фикса BUG-1820 (две cfg-строки) показывает
те же 46 `never used` — до фикса dead-code-проход там не доходил: компиляция
останавливалась на resolution-ошибках двух голых `mod` (E0432/E0425), маскируя
часть потока. Итого на `14a917d79` карточка BUG-1820 видела только 8 ошибок
`lib test` — поток таргета `lib` вырос в окне `14a917d79..e0f8aff95`
(CSSANIM-MODEL-2/3, LOAD-9, BUG-1306/1742/1818/1824, BUG-1468 с5), не бисектился.

## Причина

Инвариант семьи BUG-1804/BUG-1820: `v8-backend` не default — голый
`-p lumen-js` собирает либу без фичи, и хелперы, живущие только v8-глеймом,
превращаются в dead code. Кто-то из срезов окна добавил такие цепочки (или
снял с них cfg), не заглянув в голый clippy.

## Что делать

Бисект окна `14a917d79..e0f8aff95` по таргету `lib` без фичи → виновник →
вернуть `#[cfg(feature = "v8-backend")]` на цепочки потребления (или
`#[cfg_attr(not(feature = "v8-backend"), allow(dead_code))]` с комментарием,
если хелпер нужен обеим конфигурациям частично). Критерий закрытия: голый
`cargo clippy -p lumen-js --all-targets -- -D warnings` зелёный; с
`--features v8-backend` тоже (это выполняет оставшуюся часть критерия
BUG-1820).
