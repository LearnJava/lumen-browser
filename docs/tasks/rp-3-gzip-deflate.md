# RP-3 — HTTP gzip/deflate Content-Encoding декодер

**Developer:** P1 · **Ветка:** `p1-rp-3-gzip-deflate` · **Размер:** S · **Крейты:** `lumen-network`

> Roadmap: `ROADMAP.md` строка `RP-3` (родитель `RP`).
> Capability gap: `CAPABILITIES.md:138` — «Brotli content-decoding (⬜ no gzip/deflate HTTP decoder)».

---

## Контекст

**Самая чистая из RP-задач, без новых зависимостей.** `flate2` **уже** в депах
(`crates/network/Cargo.toml:44`, workspace). Есть готовая инфраструктура:
- trait `ContentDecoder` (`lumen-core::ext`, реэкспорт `lumen-network::lib.rs:30`);
- образец реализации `BrotliContentDecoder` (`crates/network/src/brotli.rs`);
- диспетчер `apply_content_encoding` (`lib.rs:1042`), который парсит заголовок
  `Content-Encoding` (comma-separated, в **обратном** порядке — RFC 7231 §3.1.2.2) и подбирает
  декодер по имени из реестра `&[Arc<dyn ContentDecoder>]`.

Сейчас зарегистрирован только Brotli (`BrotliContentDecoder::new()`, lib.rs:4432 и тесты), и в
`Accept-Encoding` объявляется только `br` (streaming-стратегия `StreamDecode::Brotli`, lib.rs:746).
Сайты, отдающие `gzip`/`deflate` (большинство), приходят неразжатыми → парсер видит мусор.

Задача: реализовать `GzipContentDecoder` и `DeflateContentDecoder` поверх `flate2`,
зарегистрировать их рядом с Brotli, добавить `gzip, deflate` в `Accept-Encoding`.

## Пред-запуск

- [ ] Прочитать `crates/network/src/brotli.rs` целиком — это шаблон (≈100 строк с тестами).
- [ ] Прочитать определение trait `ContentDecoder` — узнать сигнатуру (`fn encoding(&self) -> &str`
      или подобное + `fn decode(&self, input: &[u8]) -> Result<Vec<u8>>`). Grep:
      `grep -rn "trait ContentDecoder" crates/`.
- [ ] Прочитать `lib.rs:1042-1075` — как `apply_content_encoding` матчит имя кодировки с декодером.
- [ ] Прочитать `lib.rs:732-760` — `StreamDecode` и выбор стратегии по `Content-Encoding`.

## Ключевые точки (реальные file:line)

- `crates/network/src/brotli.rs:24` — `struct BrotliContentDecoder` + `impl ContentDecoder` (шаблон).
- `crates/network/src/lib.rs:70` — `pub use brotli::BrotliContentDecoder;` (добавить новые рядом).
- `crates/network/src/lib.rs:1042` — `fn apply_content_encoding(...)` (диспетчер по имени).
- `crates/network/src/lib.rs:1062` — цикл `for encoding in encodings.iter().rev()` (матч имени).
- `crates/network/src/lib.rs:746` — `"br" => StreamDecode::Brotli` (расширить gzip/deflate или
  оставить их буферными — см. ниже).
- `crates/network/src/lib.rs:4432` — `.with_content_decoder(Arc::new(BrotliContentDecoder::new()))`
  (точка регистрации в дефолтном клиенте — добавить gzip+deflate).
- где формируется `Accept-Encoding` запроса — grep `"br"` / `Accept-Encoding` в построении headers.

## Различие gzip vs deflate (важно)

- **`gzip`** — `flate2::read::GzDecoder` (RFC 1952, с gzip-заголовком/CRC).
- **`deflate`** — по HTTP-спеку это zlib-обёртка (RFC 1950) → `flate2::read::ZlibDecoder`.
  Но часть кривых серверов шлёт **raw deflate** (RFC 1951) без zlib-заголовка. Прагматично:
  сначала пробовать `ZlibDecoder`, при ошибке — фолбэк на `DeflateDecoder` (raw). Это поведение
  как у браузеров. Покрыть оба случая тестами.

## Шаги

1. Ветка + worktree (`p1-rp-3-gzip-deflate`).
2. Новый файл `crates/network/src/gzip.rs`:
   - `GzipContentDecoder` (`encoding() == "gzip"`, decode через `GzDecoder::read_to_end`);
   - `DeflateContentDecoder` (`encoding() == "deflate"`, zlib→raw фолбэк).
   - Точно скопировать форму `impl ContentDecoder` из `brotli.rs`, ошибки оборачивать в
     `Error::Other(format!("gzip decode failed: {e}"))`.
3. `lib.rs:70` — `pub use gzip::{GzipContentDecoder, DeflateContentDecoder};` + `mod gzip;`.
4. Зарегистрировать оба декодера везде, где регистрируется Brotli (минимум — дефолтный клиент
   lib.rs:4432; в тестовых `vec![Arc::new(BrotliContentDecoder::new())]` по необходимости).
5. `Accept-Encoding`: добавить `gzip, deflate` к объявляемым кодировкам (и при необходимости
   ветку в `StreamDecode` — или оставить gzip/deflate как буферное декодирование через
   `apply_content_encoding`, если streaming для них не делаем; задокументировать выбор).

## Тесты (gzip.rs, по образцу brotli.rs)

Тест-векторы получить эталонным энкодером (как в brotli.rs):
```bash
echo -n "Hello, World!" | gzip -c | xxd -i      # gzip-вектор
printf 'Hello, World!' | python3 -c "import sys,zlib;sys.stdout.buffer.write(zlib.compress(sys.stdin.buffer.read()))" | xxd -i  # zlib deflate
```
- `gzip_roundtrip_ascii` / `gzip_roundtrip_cyrillic_utf8`.
- `deflate_zlib_roundtrip` (с zlib-заголовком).
- `deflate_raw_fallback` (raw deflate без заголовка → фолбэк-ветка).
- `gzip_rejects_garbage` (битый вход → Err с «gzip decode failed»).
- Интеграция: `apply_content_encoding` с `Content-Encoding: gzip` отдаёт распакованное тело;
  `Content-Encoding: gzip` поверх уже-Brotli (chained, reverse-order) — если просто, добавить.

## Definition of done

- `gzip` и `deflate` (zlib + raw-фолбэк) распаковываются; объявлены в `Accept-Encoding`.
- Без новых зависимостей (flate2 уже есть) — в теле коммита отметить, что новый dep не добавлялся.
- `cargo clippy -p lumen-network --all-targets -- -D warnings` + `cargo test -p lumen-network` зелёные.
- `CAPABILITIES.md:138` — убрать «no gzip/deflate HTTP decoder».
- Удалить указатель `ROADMAP.md:181` из `STATUS-P1.md`; `RP-3` → `done`.
