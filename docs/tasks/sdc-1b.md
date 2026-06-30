# Задача: SDC-1b — канал команд в живое окно (shell, 8A.7 Ф4)

**Developer:** P3
**Ветка:** `p3-sdc1b-shell-command-channel`
**Размер:** M
**Крейты:** `lumen-shell`
**ТРЕБУЕТ:** SDC-1a (смержен) — командный интерфейс `AutomationCommand`/`AutomationReply` в `lumen-driver`

## Контекст
SDC-1a (P1) опубликовал в `lumen-driver` командный интерфейс `AutomationCommand` + дореализовал
методы `WinitSession`. SDC-1b — подключить этот интерфейс в **живое видимое окно** шелла, чтобы
внешний клиент дёргал команды против активной вкладки. `lumen-shell` — территория P3, поэтому
shell-проводка отдельной задачей (CLAUDE.md «interface-first»: P1 публикует контракт, P3 интегрирует).

## Пред-запуск
- [ ] Смержен SDC-1a; прочитать `AutomationCommand`/`AutomationReply` в `lumen-driver`
- [ ] Прочитать: `crates/shell/src/main.rs:105` (`enum LoadEvent`), `:274` (`EventLoopProxy`),
      `:7410+` (`impl ApplicationHandler<LoadEvent>`, `window_event`, доставка user-event в loop)
- [ ] Прочитать: `crates/shell/src/main.rs:923` (`run_ipc_server` — образец IPC-приёма через `lumen_ipc`)
- [ ] Прочитать: как окно навигирует вкладку внутри (адресная строка → загрузка), чтобы переиспользовать
- [ ] Память: `project_femtovg_default_backend.md`, `project_text_render_paths_fork.md`

## Что сделать
1. **CLI-флаг** (напр. `--remote-control` или `--ipc-server --with-window`): поднимает обычное
   femtovg-окно + слушающий поток.
2. **Слушающий поток** принимает `AutomationCommand` (через `lumen_ipc` TCP loopback) → транслирует
   в `LoadEvent` (новые варианты) → шлёт через `EventLoopProxy` в живой event-loop.
3. **Обработка в `ApplicationHandler`** (main.rs:7410): применить команду к активному табу
   (navigate/click/type/scroll/eval/screenshot/wait — вызвать соответствующую логику окна или
   методы `WinitSession`-контракта SDC-1a). Нативный ввод идёт тем же путём, что OS-события.
4. **Ответ наружу:** `AutomationReply` обратно по каналу; ack `Painted` отправлять ПОСЛЕ
   завершения `RedrawRequested` для команды (детерминизм для скриншот-потребителей).
5. Детерминизм: фиксированный 1024×720 контент-viewport; совместимость с `--no-scrollbar`.
6. Без automation-флага поведение шелла без изменений (perf-гейт ADR-006: median ≤5%, RAM ≤5%).

## Приёмка
- [ ] `lumen --remote-control` поднимает видимое окно, НЕ закрывается; по `Navigate/Click/Type/
      Scroll/Eval/Screenshot` окно реагирует, приходят `AutomationReply`/ack; серия команд подряд
      в одном процессе без подвисаний/утечек
- [ ] `cargo run -p lumen-bench --release` — без регресса дефолтной сборки
- [ ] `cargo clippy -p lumen-shell --all-targets -- -D warnings`
- [ ] `cargo test -p lumen-shell`

## Завершение
- Удалить строку `ROADMAP.md:185` из `STATUS-P3.md` (с переиндексацией пойнтеров в тот же файл)
- `ROADMAP.md` SDC-1b status → done; `python scripts/gen_roadmap.py`
- `CAPABILITIES.md:188` — отметить живую shell-интеграцию automation
- Удалить этот файл. SDC-2 (фронты) и SDC-3 (графтест) разблокированы.
