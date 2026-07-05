# Задача: AVIF / JPEG XL декодирование

**Developer:** P1
**Ветка:** `p1-avif`
**Размер:** M
**Крейты:** `lumen-image`

## Goal

Сделать AVIF декодируемым в дефолтной сборке (сейчас — только за feature-флагом
`avif`, требующим cmake+nasm) и реализовать реальный декодер JPEG XL (сейчас —
заглушка, всегда `Err`). Оба формата должны попасть в
`supported_mime_types()` и корректно отдавать RGBA8 в pipeline изображений.

## Current state (сверено с кодом 2026-07-05)

- `crates/engine/image/src/avif/mod.rs:68` — `decode_avif`: **реальный** декодер,
  но `decode_avif_impl` за `#[cfg(feature = "avif")]` (`avif/mod.rs:75`). Без
  фичи — `#[cfg(not)]`-ветка (`avif/mod.rs:85`) возвращает `AvifError::Decode`.
  `is_avif` (`avif/mod.rs:47`) работает всегда (sniff по ftyp brand avif/avis).
  `AvifImageDecoder` (`avif/mod.rs:96`) реализует `ImageDecoder`.
- `crates/engine/image/Cargo.toml:13` — feature `avif = ["dep:image"]`,
  `image = "0.25"` (features `["avif"]`) **optional** (`Cargo.toml:28`).
  В дефолтной сборке подключается только НЕ `image` — AVIF не декодит.
- `crates/engine/image/src/jxl.rs:70` — `decode_jxl`: **чистая заглушка**,
  всегда `Err(JxlError)` (`jxl.rs:71`). `is_jxl` (`jxl.rs:32`) — рабочий sniff
  (naked `FF 0A` + ISOBMFF `jxl ` brand). НЕТ `JxlImageDecoder` (в отличие от
  AVIF нет реализации `ImageDecoder`).
- `crates/engine/image/src/lib.rs:130` — диспетчер `decode()`: `is_avif` →
  `decode_avif` (`lib.rs:131`); `is_jxl` → сразу `Err(ImageError::Jxl(...))`
  (`lib.rs:135`).
- `crates/engine/image/src/lib.rs:36,45` — `image/avif` есть в
  `supported_mime_types()`; `image/jxl` **исключён** намеренно (`lib.rs:1218`
  тест `supported_mime_types_excludes_jxl_stub`), т.к. декодер-заглушка.
- Зависимостей на JPEG XL (`jxl-oxide`/`libjxl`) в `Cargo.toml` **нет** — новая
  зависимость требуется.

## Entry points

- `crates/engine/image/src/avif/mod.rs:75` — `decode_avif_impl` (feature-gated).
- `crates/engine/image/src/jxl.rs:70` — `decode_jxl` (заглушка → реализовать).
- `crates/engine/image/src/lib.rs:130` — диспетчер форматов.
- `crates/engine/image/src/lib.rs:36` — `supported_mime_types()`.
- `crates/engine/image/Cargo.toml:10` — секции `[features]` / `[dependencies]`.

## Срезы (декомпозиция)

### Срез 1 — S — Решение по AVIF-дефолту (декодер без cmake/nasm)
Текущий путь (`image`+libavif) требует cmake+nasm — тяжёлая нативная сборка,
поэтому за флагом. Оценить чистый-Rust AV1-декодер: `dav1d` (тоже C),
`rav1e` (энкодер) не подходит; кандидат — `image` с `avif-native` vs
`libavif-rs`. Если чистый-Rust AVIF-декодер отсутствует/незрелый — оставить
за фичей и явно зафиксировать в DoD (feature `avif` не выключаема без cmake).
Задокументировать выбор в теле коммита (**Why this dependency**).

### Срез 2 — S — AVIF в дефолтную сборку (если срез 1 дал зелёный)
Если найден приемлемый декодер: сделать `avif` не-optional или дефолтной фичей
(`Cargo.toml:13`), убрать `#[cfg(feature)]`-ветвление в `avif/mod.rs:75/85` (или
оставить как единственный путь). Обновить `sniff`→`decode` без флага.

### Срез 3 — M — JPEG XL: новая зависимость + декодер
Добавить `jxl-oxide` (чистый Rust JXL-декодер) в `Cargo.toml`. Реализовать
`decode_jxl` (`jxl.rs:70`): naked + ISOBMFF-контейнер → RGBA8. Обработать
ошибки в `JxlError`. Записать **Why this dependency** (provisional,
trait-anchor `ImageDecoder`, graduation — когда JXL станет обязательным).

### Срез 4 — S — `JxlImageDecoder` + регистрация в диспетчере
По образцу `AvifImageDecoder` (`avif/mod.rs:96`) добавить `JxlImageDecoder`
(`ImageDecoder` для `image/jxl`). Включить `image/jxl` в
`supported_mime_types()` (`lib.rs:36`) и убрать/переписать тест-инвариант
`supported_mime_types_excludes_jxl_stub` (`lib.rs:1218`).

### Срез 5 — XS — ICC-профиль (опционально)
`avif/mod.rs:9` и `lib.rs:458` помечают ICC как отложенный (Phase 1). Если
декодер отдаёт ICC — прокинуть в поле `icc_profile` (иначе оставить `None` и
пометить как отдельный follow-up, не в DoD).

## Tests

- Юнит `crates/engine/image/src/avif/mod.rs` (mod tests, `avif/mod.rs:116`):
  добавить декод реального минимального AVIF (не мусор) → корректные w/h/RGBA.
- Юнит `crates/engine/image/src/jxl.rs` (mod tests, `jxl.rs:74`): заменить
  `test_decode_jxl_always_fails` (`jxl.rs:124`) на декод реального JXL-семпла.
- Юнит `crates/engine/image/src/lib.rs`: `jxl_signature_dispatches_to_decoder`
  (симметрично `avif_signature_dispatches_to_avif_decoder`, `lib.rs:955`);
  `supported_mime_types_includes_jxl`.
- Тестовые семплы: минимальные `.avif` / `.jxl` в тест-ресурсах крейта (embed
  байтами или `include_bytes!`), не в `graphic_tests/`.

## Definition of done

- [ ] Решение по AVIF-дефолту принято и зафиксировано (декодер без cmake/nasm ИЛИ явно оставлен за feature `avif` с обоснованием).
- [ ] `decode_jxl` реально декодирует JPEG XL (naked + ISOBMFF) в RGBA8.
- [ ] `JxlImageDecoder` реализует `ImageDecoder`, `image/jxl` в `supported_mime_types()`.
- [ ] Новая зависимость (`jxl-oxide` и/или AVIF-декодер) обоснована в коммите (**Why this dependency**) + строка в `docs/plan/tech-stack.md`.
- [ ] Юниты с реальными AVIF/JXL семплами зелёные.
- [ ] `CAPABILITIES.md` + `subsystems/image.md` обновлены (AVIF/JXL ✅/🟡).
