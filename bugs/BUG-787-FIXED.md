# BUG-787 — `lumen_image::decode` зависает навсегда на 111-байтном GIF (LZW-распаковка кадра не завершается)

**Статус:** FIXED 2026-08-20
**Заведён:** 2026-08-19 (TEST-1, первый реальный прогон cargo-fuzz — CI-джоб `fuzz`)
**Область:** image (`crates/engine/image/src/gif.rs` — `AnimatedGif::frame_image`, вызов `gif::Frames::decode_lzw_encoded_frame_into_buffer`)
**Владелец:** P3 (движок). Заведён P2 в ходе тулинговой задачи, исправлен P3 2026-08-20.

## Симптом

`lumen_image::decode(bytes)` на 111 байтах не возвращает управление.
Ограничение сверху не установлено: процесс крутился **>60 с** в
`dev-release`-сборке на Windows и был убит; в CI libFuzzer убил его по
собственному лимиту — `ERROR: libFuzzer: timeout after 29 seconds`.

Вход — синтаксически правдоподобный GIF89a 1×1, найденный фаззером из
курированного seed-корпуса за ~60 с работы:

```
00000000: 4749 4638 3961 0100 0100 8000 0000 7f00  GIF89a..........
00000010: 0000 0021 f904 6007 00ff 002c 0000 0000  ...!..`....,....
00000020: 0100 0100 0002 0235 4401 2020 2020 2020  .......5D.
00000030: 2020 2020 5a20 2020 2020 2020 2020 2020      Z
```

**Минимизированное репро — 78 байт** (`cargo fuzz tmin`, дальше не ужимается:
«failed to minimize beyond … (78 bytes)»). Раскодировать `base64 -d`:

```
R0lGODlhAQABAIAAAAB/AAAAACH5BGAHAP8ALAAAAAABAAEAAAICNUQBICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgIAA7
```

Разбор: `GIF89a`, логический экран 1×1 с глобальной палитрой, Graphic Control
Extension (`21 F9 04 …`), image descriptor 1×1 (`2C`), LZW minimum code size
`02`, один подблок из двух байт (`35 44`), затем подблок `01 20`, дальше
33 байта `0x20`, терминатор блоков `00` и трейлер `3B`. То есть картинка
объявлена как один пиксель, а LZW-поток за ней — мусор; именно на нём
распаковщик и не возвращается.

Исходное (неминимизированное, 111 байт) — артефакт `fuzz-artifacts-32274370138` прогона
[32274370138](https://github.com/LearnJava/lumen-browser/actions/runs/32274370138)
(`gh run download 32274370138 -n fuzz-artifacts-32274370138`), имя файла
`timeout-ca30162df0d5faa438184bb16e4771da977fe0d0`. В `fuzz/regressions/`
он намеренно **не** закоммичен: replay-шаг CI проигрывает этот каталог на
каждом прогоне, и до фикса это повесило бы джоб на 90 минут вместо того,
чтобы отчитаться о находке. Класть его туда — вместе с фиксом.

## Механизм (локализовано пробами, не предположение)

Три пробы на одном и том же вводе, каждая — временный тест в
`crates/engine/image/src/gif.rs`, прогон `--profile dev-release`:

1. **Метаданный проход не при чём.** Цикл `next_frame_info()` в
   `decode_gif_animated` завершается за **2 мкс**, найдя ровно 1 кадр;
   `decode_gif_animated` целиком отрабатывает и отдаёт `AnimatedGif`
   1×1, `frames=1`, `loop=Finite(0)`.
2. **Зависает `frame_image(0)`.** Он же — единственное, что делает
   `decode_gif` (`gif.rs:357-359`: `decode_gif_animated(bytes)?.frame_image(0)`).
3. **Внутри `frame_image` — не наш цикл.** `cursor.reader.read_next_frame()`
   возвращает `Some(frame)` за **1.04 мс**; управление не возвращается из
   следующего вызова — `cursor.frames.decode_lzw_encoded_frame_into_buffer(frame,
   &mut buffer)` (`gif.rs:317-320`), то есть из LZW-распаковки в крейте `gif`
   0.14.2 (LZW — `weezl` 0.1.12). Целевой буфер здесь 1×1×4 = **4 байта**.

Цикл `while cursor.next_idx <= idx` в `frame_image` сам по себе корректен
(есть `break` на `None`, `next_idx` растёт) — зависание происходит **внутри**
одной итерации.

## Почему это важно

Класс — DoS: любая страница с таким GIF подвешивает декод. Путь
пользовательский, не тестовый: `decode` вызывается на сетевых ресурсах
`<img>`. Никакого предела на время/объём работы декодера у нас нет — ни
таймаута, ни лимита на число выходных пикселей относительно размера входа.

## Направления фикса (для владельца — не выбор, а материал)

- Проверить, воспроизводится ли на свежем `gif`/`weezl` (наш `gif` 0.14.2,
  `weezl` 0.1.12) — если это известный upstream-баг, обновление крейта и есть
  фикс, а нижние пункты становятся страховкой.
- Ограничить работу декодера сверху на нашей стороне: кадр 1×1 не может
  требовать сколько-нибудь заметного времени, а `decode_lzw_encoded_frame_into_buffer`
  получает буфер известного размера — верхняя граница на объём распаковываемых
  данных выводится из `width*height`, и её нарушение должно давать
  `GifError::DecodeError`, а не бесконечную работу.
- Дальний родственник — [BUG-396](BUG-396-FIXED.md) (тот же файл, тот же
  LZW-путь: спек-валидный кадр без GCE отклонялся как «invalid code in LZW
  stream»). Тот баг был про ложный отказ, этот — про отсутствие отказа вовсе.

## Регрессионный тест

После фикса: положить репро в `fuzz/regressions/fuzz_image-gif-lzw-hang`
(имя обязано начинаться с имени таргета — CI-шаг replay сверяет префикс со
списком `cargo fuzz list`) и добавить его же в `fuzz/corpus/fuzz_image/`.
Отдельно — юнит-тест в `gif.rs`, проверяющий, что `decode` на этом вводе
возвращает `Err` за разумное время.

---

## Фикс (2026-08-20, P3)

### Корень — в апстриме, и он не обходится настройкой

`gif` 0.14.2, `reader/decoder.rs:257-263`:

```rust
self.pixel_converter.read_into_buffer(frame, buf, &mut move |out| loop {
    let (bytes_read, bytes_written, status) = lzw_reader.decode_bytes(data, out)?;
    data = data.get(bytes_read..).unwrap_or_default();
    if bytes_written > 0 || matches!(status, LzwStatus::NoProgress) {
        return Ok(bytes_written);
    }
})?;
```

Выход из цикла — только «что-то записано» либо `NoProgress`. А `weezl` 0.1.12
после end-кода уходит в `has_ended` и на любой вход отвечает
`(consumed_in: 0, consumed_out: 0, status: Done)` — не `NoProgress`. Кадр, чей
LZW-поток кончился (или начался с end-кода, как в этом репро: `35 44` при
`min_code_size = 2` — это код 5 = END при `CLEAR = 4`) раньше, чем заполнен
кадровый буфер, крутит этот `loop` вечно. Никаких опций `DecodeOptions`,
меняющих условие выхода, у крейта нет; 0.14.2 — последняя версия на crates.io
на 2026-08-20, обновлением это не лечится.

### Что сделано

Пиксельный проход перестал ходить в `gif::FrameDecoder`
(`crates/engine/image/src/gif.rs`):

- `lzw_decode_into` — свой цикл поверх `weezl::decode::Decoder::decode_bytes`
  с тремя условиями остановки сразу: буфер кадра заполнен · декодер вернул не
  `Ok` (`Done`/`NoProgress`) · итерация не сдвинула ни вход, ни выход. Входной
  срез монотонно укорачивается, так что цикл конечен при любом содержимом.
- `decode_frame_rgba` — палитра кадра (или глобальная), transparent-index и
  deinterlace (`interlace_row_order`, GIF spec §20.c.ii). Поведение
  побайтно повторяет конвертер `gif` (`reader/converter.rs:139-160`), включая
  «индекс, которого нет в палитре, → пиксель не трогаем» и игнор
  `frame.left`/`frame.top` ([BUG-763](BUG-763-OPEN.md) — отдельный баг, здесь
  не чинится).
- Оборванный поток (`written < width × height`) даёт
  `GifError::DecodeError("LZW-поток кадра оборван: N из M пикселей")`.
  Это ужесточение: раньше такой кадр вешал процесс, теперь картинка не
  декодируется вовсе. Частичный кадр не показываем — у нас нет прогрессивного
  показа, для которого он был бы полезен.
- `GifCursor` вместо `frames: Box<FrameDecoder>` держит `global_palette:
  Option<Vec<u8>>`; `weezl` стал прямой зависимостью `lumen-image`
  (транзитивно он и так был — через `gif`).
- Свойство «хвост потока за последним пикселем не читаем», ради которого был
  сделан [BUG-396](BUG-396-FIXED.md), сохранено: цикл выходит по заполнению
  буфера раньше, чем смотрит на статус.

### Тесты

- `bug787_frame_with_truncated_lzw_stream_errors_instead_of_hanging` —
  минимизированное 78-байтное репро (константа в тесте побайтно равна
  `fuzz/regressions/fuzz_image-gif-lzw-hang`): контейнерный проход по-прежнему
  видит кадр 1×1, а `frame_image(0)` и `decode` **возвращаются** с
  `DecodeError`.
- `interlace_row_order_matches_spec_passes`, `interlaced_frame_rows_land_in_screen_order`
  — порядок строк чересстрочного кадра (своя реализация вместо
  `gif::reader::converter::InterlaceIterator`).
- `frame_pixel_outside_palette_stays_transparent` — индекс вне палитры остаётся
  прозрачным. Тонкость: палитру меньше двух записей брать нельзя — `gif::Encoder`
  дополняет её до степени двойки, и «отсутствующий» индекс попал бы в дописанный
  чёрный цвет.
- Репро положено в `fuzz/regressions/fuzz_image-gif-lzw-hang` и
  `fuzz/corpus/fuzz_image/gif-lzw-ends-early.gif`; `fuzz_image` убран из
  `KNOWN_FAILING` в `.github/workflows/fuzz.yml` — таргет снова блокирующий, а
  шаг replay гоняет это репро на каждом прогоне.
