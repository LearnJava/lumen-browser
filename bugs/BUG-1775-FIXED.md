# BUG-1775 — windows-1252 в query: `url("?ÿ")` не кодируется как `?%FF` после BUG-1713 — query-encoding 17/26→2/26

**Статус:** FIXED 2026-11-09
**Заведён:** 2026-10-28 (P2, WPT-DROP-1027-S3)
**Область:** encoding / shell (`crates/engine/encoding/src/lib.rs`, `tables.rs`; вызов кодировки query при разборе URL документа)

## Симптом

Страница с `?encoding=windows-1252` (метка из `<meta charset>`/заголовка) должна кодировать query
своих URL в windows-1252: `?ÿ` → `?%FF`. С `a51331ac5` getComputedStyle отдаёт `url("?ÿ")` без
процентного кодирования — подтест «getComputedStyle <body background> — url("?ÿ") did not end with ?%FF».

| id | `e139bd18e` (до) | `a51331ac5` (после) | текущий `main` |
|---|---|---|---|
| `…/query-encoding/attributes.sub.html?encoding=windows-1252` | 17/26 | 2/26 | 2/26 |
| `…/lone-surrogates.sub.html?encoding=windows-1252` | 1/1 | 0/1 | 0/1 |
| `…/navigation.sub.html?encoding=windows-1252` | 2/6 | 0/6 | 0/6 |

## Бисект (WPT-DROP-1027-S3)

`git bisect --first-parent`, окно `89500a780`(ok)…`bf47a575d`(bad), 3 сборки: `a51331ac5` bad,
`e139bd18e` ok. Виновник — `a51331ac5` «Влить ветку p6-bug-1713», единственный коммит кода
`99c91124b` («TextDecoder и документы понимают метки windows-1252 (ascii/latin1/iso-8859-1)»):
правки `crates/engine/encoding/src/{lib.rs,tables.rs}`, `crates/js/src/shim/text_encoding_shim.js`.
Метка windows-1252 теперь распознаётся, но путь query-кодирования для неё не получает
кодировщик (query остаётся в «сыром» виде) либо возвращает не те байты.

## Воспроизведение

`tests/wpt/run_smoke.py --binary <lumen.exe> "/html/infrastructure/urls/resolving-urls/query-encoding/attributes.sub.html?encoding=windows-1252"`
(бинари: `.tmp/bisect-bin/{e139bd18e,a51331ac5}/lumen.exe` слота p2-work, запускать на
`lumen.py` из `a76e4c7b9`). Ожидание: 17/26 как до `a51331ac5`; факт: 2/26.

## Фикс (2026-11-09, P3)

Причина глубже, чем «кодировщик не получает метку»: после `99c91124b` `document.characterSet`
честно отвечает `windows-1252`, но все пути резолва URL остались чисто UTF-8 (url crate в
отражении атрибутов, навигации, геттерах) или сырыми (computed `background-image` сериализовал
строку атрибута как есть) — падали все 24 кодировочно-чувствительных подтеста, проходили только
2 строковых `ping`.

Фикс — «encoding parse a URL» (HTML LS §2.1) тремя звеньями:

- `lumen_encoding::encode_url_query(href, enc)` — пре-кодировщик query ДО разбора url crate:
  не-ASCII → `%XX` по таблице кодировки документа, неэнкодируемое — numeric character
  reference (`%26%23N%3B`; lone surrogate → U+FFFD → `%26%2365533%3B`); ASCII и готовые `%XX`
  не трогаются; `?` внутри fragment (`page#f?ÿ`) пропускается; UTF-8/16/32 — no-op. Плюс
  `encode_byte` — однобайтовый энкодер WIN1251/WIN1252/KOI8_R/CP866 (первый индекс, U+FFFD
  не матчится).
- Натив `_lumen_url_resolve_doc` (charset из захваченного `Arc<Document>`), JS-обёртка
  `_url_resolve_doc` с фолбэком `_url_resolve` (воркеры).
- Подключено в `_lumen_reflect_url` (отражение URL-атрибутов), `_lumen_navigate_or_fragment`,
  геттер `<base>.href`, `resolveUrl`/track-fetch `video_element.js`, сериализацию computed
  `background-image` (атрибут `background`) в обоих путях чтения (readProp и entries).
  `new URL()` остаётся чистым UTF-8 — конструктор не наследует кодировку документа.

| id | до фикса | после фикса |
|---|---|---|
| `attributes.sub.html?encoding=windows-1252` | 2/26 | **25/26** (до регрессии `e139bd18e` было 17/26) |
| `lone-surrogates.sub.html?encoding=windows-1252` | 0/1 | **1/1** |
| `navigation.sub.html?encoding=windows-1252` | 0/6 | TIMEOUT — и utf-8-вариант таймаутится на том же бинарнике: до-существующий провал навигационного харнесса, не регрессия фикса |

Остаточные пробелы (отдельные дефекты, не этот баг): `Getting <audio>.src` — геттер `src`
аудио-шима возвращает `_src`, снятый в момент патча, и не обновляется `setAttribute` (пустая
строка) — дефект отражения, не кодирования. Сетевой фетч сабресурсов резолвится на стороне Rust
в UTF-8 — doc-кодировкой кодируется JS-видимое отражение.
