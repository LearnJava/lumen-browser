# BUG-1807 — shell: устаревший индекс фрейма в `about_to_wait.rs::frame_dirty` — паника `index out of bounds` в `relayout_frame_content`

**Статус:** OPEN
**Открыт:** 2026-11-14 (P3, найден при живой сверке BUG-1800 срез 6)
**Компонент:** shell (`crates/shell/src/app/about_to_wait.rs:173-236` — `frame_dirty`; `crates/shell/src/frames.rs:1565` — `relayout_frame_content`)

## Симптом

auto.ru, `LUMEN_VERIFY_INCR_CASCADE=1` (любой бинарь: и ветка BUG-1800 срез 6, и **чистый main `5e3daec4a`**):

```
thread 'lumen-browser' panicked at crates\shell\src\frames.rs:1565:23:
index out of bounds: the len is 5 but the index is 5
  3: lumen::frames::relayout_frame_content
  4: <Lumen>::on_resumed
  5: <Lumen>::on_about_to_wait
```

Окно умирает (процесс выходит), на auto.ru — за 20–180 с сессии.

## Причина

`about_to_wait.rs` собирает `frame_dirty: Vec<usize>` по `frame_js_handles` (строка 173–217),
затем вызывает `self.poll_dynamic_frames()` (строка 232) — тот спавнит **и выгружает**
динамические iframe-фреймы (`frame_lazy`), состав `self.frames` меняется, — и только после
этого идёт `for idx in frame_dirty { self.refresh_frames(Some(idx)) }` →
`relayout_frame_content(&mut frames, idx, …)` → `frames[idx]` с индексом, который мог
пережить удаление фрейма (len 5, idx 5). Комментарий на месте (строки 227–230) обещает
«refresh_frames не трогает состав self.frames» — про `poll_dynamic_frames` между сбором
и использованием обещание ничего не говорит.

## Репро

```
$env:LUMEN_VERIFY_INCR_CASCADE = "1"; $env:LUMEN_NO_ADBLOCK = "1"
lumen.exe https://auto.ru/ --maximized
```

Краш воспроизведён на чистом main `5e3daec4a` (сборка dev-release 2026-11-14, ~2 мин сессии)
и на ветке BUG-1800 срез 6. Без verify-env за A/B-прогоны срезов 5–6 (десятки сессий)
не встречался — окно гонки открывают медленные флаши (полный каскад на каждый флаш) и/или
частая выгрузка рекламных iframe.

## Что сделать

Не держать индексы через мутирующий состав: либо дренаж `frame_dirty` до
`poll_dynamic_frames`, либо идентификатор фрейма вместо позиции (`FrameHandle` имеет
стабильный ключ? — проверить), либо повторная проверка `idx < frames.len()` с отбросом
устаревших. Заодно проверить соседние `frame_scrolls`/`frame_submits`/`frame_reloads`
(та же схема «индексы → мутирующий вызов → использование»).

Найдено P3 при живой сверке `LUMEN_VERIFY_INCR_CASCADE` (BUG-1800 срез 6).
