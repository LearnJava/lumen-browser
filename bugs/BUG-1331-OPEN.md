# BUG-1331 — `word-break: normal` не режет тайский, кхмерский и др. (класс SA UAX #14) по словарю

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/line_break.rs`, `inline_wrap.rs` — для класса SA возможностей переноса нет; `lang` не используется)

## Симптом

`width: 100px; font: 20px monospace`, `lang=th`, строка из ≈40 тайских символов (`สวัสดีครับผมชื่อสมชายยินดีที่ได้รู้จัก`): `--dump-layout` — один блок высотой 23,42 (одна строка), один фрагмент. То же для кхмерского (`lang=km`). Режется только по пробелам и по CJK-таблице.

## Как найдено

WPT-RUN-14 срез 10: `word-break/word-break-normal-{th,km,lo,my,bo,tdd,hi,ko,en}-000.html`, `word-break-normal-ethiopic.html`, `-001/-002/-003.html`, `word-break-normal-th-001.html` — 14 id (11 `thick`, 3 `no-match-ref`). Пробой подтверждены только th и km; для `en`/`hi`/`ko`/`ethiopic` словарь не нужен, причина не установлена.

## Что делать

Словарная сегментация SA (Thai/Lao/Khmer/Myanmar): сторонний крейт-сегментатор или собственный словарь — решение по ADR-027. Задача уровня `LINEBREAK-UAX14`, а не правка.

## Как проверить

`css/css-text/word-break/word-break-normal-th-000.html`.
