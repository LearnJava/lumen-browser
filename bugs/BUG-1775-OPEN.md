# BUG-1775 — windows-1252 в query: `url("?ÿ")` не кодируется как `?%FF` после BUG-1713 — query-encoding 17/26→2/26

**Статус:** OPEN
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
