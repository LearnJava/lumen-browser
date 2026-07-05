# Задача: Compression Streams (gzip/deflate/deflate-raw)

**Developer:** P1
**Ветка:** `p1-compression-streams`
**Размер:** S
**Крейты:** `lumen-js`

## Goal
Полная поддержка WHATWG Compression Streams (`CompressionStream`/`DecompressionStream`)
для форматов `deflate-raw`, `deflate`, `gzip` согласно https://compression.spec.whatwg.org/.

## Current state (сверено с кодом 2026-07-05)
**СЕМЯ В ROADMAP УСТАРЕЛО.** Строка `ROADMAP.md:156` утверждает «throw для всех форматов» —
это НЕ так. По факту фича практически ЗАКРЫТА:

- `crates/js/src/dom.rs:7382-7435` — JS-шим `CompressionStream`/`DecompressionStream`
  реализован поверх `TransformStream` (buffer-then-flush модель): накапливает чанки,
  на `flush()` атомарно (де)компрессит и эмитит один `Uint8Array`.
- `crates/js/src/dom.rs:2860-2920` — нативные биндинги `_lumen_compress_bytes` /
  `_lumen_decompress_bytes` через `flate2` (`DeflateEncoder`/`ZlibEncoder`/`GzEncoder`
  и симметричные декодеры). Реально работают для всех трёх форматов.
- `crates/js/src/dom.rs:7387` — `_COMPRESSION_FORMATS = ['deflate-raw','deflate','gzip']`;
  неизвестный формат → `TypeError` (spec-корректно).
- Тесты: `crates/js/src/dom.rs:24055-24210` — конструктор, TypeError на unsupported,
  readable/writable = ReadableStream/WritableStream, instanceof TransformStream,
  round-trip gzip / deflate / deflate-raw. Все проходят.

**Что важно про формат:** WHATWG-спек включает ТОЛЬКО `deflate-raw`/`deflate`/`gzip`.
**Brotli НЕ входит в спецификацию** Compression Streams (недавно добавлен только `zstd`,
и то не всеми движками). Заголовок задачи в ROADMAP («gzip/deflate/brotli») ошибочен —
brotli реализовывать в CompressionStream не нужно и запрещено спекой.

## Entry points
- `crates/js/src/dom.rs:7382` — JS-шим Compression/Decompression Streams.
- `crates/js/src/dom.rs:2863` — нативный `_lumen_compress_bytes`.
- `crates/js/src/dom.rs:2895` — нативный `_lumen_decompress_bytes`.
- `crates/network/src/flate.rs` / `crates/network/src/brotli.rs` — HTTP-декодеры
  (для справки; на них Compression Streams не завязаны, у шима свои flate2-биндинги).

## Срезы (декомпозиция)
### Срез 1 — XS — Приёмка текущего состояния
Прогнать `cargo test -p lumen-js compression_stream` и убедиться, что все round-trip
тесты зелёные. Если да — фича закрыта, остаются только доводки ниже.

### Срез 2 — XS — Стриминговая (де)компрессия вместо buffer-then-flush
Текущая модель копит ВСЕ чанки и жмёт разом на `flush` (пик-RSS = размер всего тела).
Спека допускает per-chunk выдачу. Опционально: заменить накопление на инкрементальный
`flate2` стрим-энкодер, эмитящий выход по мере `transform(chunk)`. Только если нужен
стриминг больших тел; иначе — не трогать (текущее поведение spec-конформно по результату).

### Срез 3 — XS — Корректная обработка ошибок декомпрессии
Сейчас `_lumen_decompress_bytes` глотает ошибку (`.ok()`) и возвращает пустой `Vec`.
По спеке битый вход должен `error` стрима (reject у reader). Дошить: различать
«пустой результат» и «ошибка формата», прокидывать в `controller.error()`.

### Срез 4 — XS — Обновить доки
`CAPABILITIES.md` (JS/Streams) → ✅; поправить строку `ROADMAP.md:156` (убрать «throw»,
убрать «brotli»); `subsystems/js.md` — отметить готовность.

## Tests
- Юнит (`lumen-js`): round-trip для трёх форматов (уже есть, `dom.rs:24118-24210`).
- Добавить: декомпрессия заведомо битых байтов → стрим переходит в errored (срез 3).
- Добавить: многочанковый ввод (несколько `write` до `close`) даёт тот же результат,
  что и один большой чанк.

## Definition of done
- [ ] Round-trip тесты gzip/deflate/deflate-raw зелёные (приёмка).
- [ ] (опц.) Битый вход в DecompressionStream → errored-стрим, а не пустой чанк.
- [ ] `ROADMAP.md` строка исправлена (не «throw», без brotli).
- [ ] `CAPABILITIES.md` / `subsystems/js.md` отмечают ✅.
- [ ] Явно зафиксировано в описании коммита: brotli вне спеки Compression Streams.
