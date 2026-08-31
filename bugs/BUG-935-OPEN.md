# BUG-935 — M4 incremental restyle for the rAF-dirty pump is dead code under the default (engine-thread-on) build; a page with an active rAF+DOM-mutation loop pays a full off-thread cascade recompute on every tick

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/relayout.rs` — `relayout_raf_dirty`, `relayout_raf_dirty_readback`, `submit_relayout_job`, `try_relayout_raf_incremental`)
**Найден:** 2026-09-01 (P3), при ревизии BUG-286 (DEBTOR, `content-visibility:auto` scroll stopor) — свежий живой замер на ria.ru показал не одну паузу ~580мс, а 33 полных off-thread relayout'а (по 850–2600мс каждый) за ~15–20с сессии, все при неизменном `dl=1690 styled=1744` (без роста DOM)

## Симптом

Живой прогон (`--mcp-live-port`, `LUMEN_PROFILE_TREE=1`+`LUMEN_FRAME_LOG=1`,
метод BUG-286 «Замер 2026-08-06») на https://ria.ru, повторённый 2026-09-01
на свежем `dev-release`: **33** записи `[engine] relayout Nms (off-thread)
dl=1690 styled=1744` за один запуск (9 — ещё до первого MCP `scroll`-вызова,
во время начальной загрузки/settle; 24 — во время и после 20 тиков скролла).
Все с одинаковым `dl`/`styled` — размер DOM не растёт, т.е. это не
раскрытие `content-visibility` (тот срабатывает один раз и увеличивает
`styled`), а нечто, дёргающее relayout непрерывно на одной и той же
странице.

Соответствующие MCP `scroll`-вызовы получали RTT 850–1400мс вместо
ожидаемых 1–5мс (BUG-286's 2026-08-06 замер: «scroll держит 2-6мс без
повторов») — round-trip блокируется, пока идёт relayout (вероятно,
блокировка на `Document`-мьютексе, которым relayout тоже пользуется через
свой снимок).

## Корень

`ria.ru` держит на главной странице `videojs`-плеер (лог содержит `[JS
error] VIDEOJS: ERROR: ...` — сам плеер не критичен, но подтверждает его
активность), который гоняет `requestAnimationFrame`-цикл и на каждом тике
трогает DOM (UI-контролы плеера). Каждый такой тик помечает `dom_dirty` и
вызывает `Lumen::relayout_raf_dirty()` (`about_to_wait.rs:392`).

`relayout_raf_dirty` (`relayout.rs:337`):

```rust
pub(crate) fn relayout_raf_dirty(&mut self) {
    if !self.submit_relayout_job() && !self.try_relayout_raf_incremental() {
        self.relayout();
    }
}
```

`&&` короткого замыкания: `submit_relayout_job()` вызывается первым и, пока
движковый поток жив (`LUMEN_ENGINE_THREAD=1` — дефолт с ADR-023,
2026-07-28), **всегда** возвращает `true` (единственное условие отказа —
отсутствие `LayoutSource`/вырожденный viewport). Поэтому `||`-ветка с
`try_relayout_raf_incremental()` — M4-инкрементальный путь
(`layout_mutation_incremental`/`relayout_page_incremental_restyle`,
переиспользующий `self.layout_box` вместо полного `compute_layout`) —
**никогда не выполняется на дефолтной сборке**. Симметричная функция
`relayout_raf_dirty_readback` (`relayout.rs:486`, RedrawRequested-путь)
устроена так же: `readback_relayout_job()` (тоже полный, тоже
приоритетный, пока движковый поток жив) закрывает `&&` раньше, чем
инкрементальный путь успевает сработать.

Старый doc-комментарий `relayout_raf_dirty` утверждал обратное — «tries the
incremental path first ... before falling back to ... submit_relayout_job
(engine thread)» (не совпадало с кодом ни в одном билде; поправлен в этой
же сессии, см. «Что уже сделано» ниже). Сестринская функция
`relayout_raf_dirty_readback` документирована **корректно** («in the
single-thread fallback path, tries the incremental layout ... before the
full relayout») — т.е. везде, где движковый поток включён (дефолт),
`try_relayout_raf_incremental` — код для `LUMEN_NO_ENGINE_THREAD=1`, а не
для основной конфигурации.

`make_relayout_job` (`relayout.rs:726`, тело задания `submit_relayout_job`)
зовёт `compute_layout(&document, &stylesheet, viewport, ...)` — полный
пересчёт каскада для ВСЕГО документа (та же стоимость, что и корень
BUG-286: `docs/tasks/p3-cascade-perf.md` «Задача 3»), просто исполняемый
асинхронно на движковом потоке вместо UI-потока. Async ⇒ UI не блокируется
(картинка не подвисает), но движковый поток остаётся занят полным
пересчётом на каждый rAF-тик с DOM-мутацией — на странице с активным
rAF+DOM-цикл (видеоплеер, карусель, «живая лента» и т.п.) это превращается
в непрерывный, а не разовый расход: пока цикл идёт, движковый поток почти
всегда занят одним relayout'ом и сразу берёт следующий (сообщение
`EngineThread::submit`'а coalescing'ит промежуточные заявки — очередь не
растёт бесконечно, — но полезная работа M4 всё равно теряется: каждый
принятый к исполнению relayout всё равно **полный**, не инкрементальный).

## Почему не тот же баг, что BUG-286

BUG-286 — про **маршрутизацию** (`self.relayout()` → `self.relayout_raf_dirty()`
для конкретного триггера `content-visibility` relevance) и её конечный вывод
был «одна пауза ~580мс, повторный full recompute сайт не запрашивает».
Замер 2026-09-01 подтверждает, что маршрутизация content-visibility сама по
себе не сломана (никакого регресса в её логике нет), но показывает, что
**тот же** `relayout_raf_dirty`, куда BUG-286 маршрутизировал триггер,
обслуживает и rAF-DOM-мутации — и там M4-инкрементальный путь,
специально построенный, чтобы такие мутации не стоили полного пересчёта
каскада, не работает на дефолтной сборке. BUG-286 остаётся закрытым в
своём скоупе (маршрутизация верна); это отдельная, точно
локализованная находка.

## Что уже сделано в этой сессии (P3, 2026-09-01)

Поправлен doc-комментарий `relayout_raf_dirty` (`relayout.rs:334-338`) —
описывал не тот порядок вызовов, который реально исполняется; приведён в
соответствие с кодом и с формулировкой сестринской функции.

## Предполагаемый фикс (НЕ сделан этой сессией — нужен перф-замер, не точечная правка)

Наивно — поменять порядок в обеих функциях местами
(`try_relayout_raf_incremental` до `submit_relayout_job`/`readback_relayout_job`),
чтобы M4 реально работал под дефолтной сборкой. Это меняет задокументированный
инвариант «async-safe триггер уходит off-thread, UI-поток не блокируется» —
`try_relayout_raf_incremental` синхронный и исполняется НА UI-потоке (мутирует
`self.layout_box`/`self.page_prev_cascade_styles`, которые UI-поток
единолично владеет), т.е. переключение сделает rAF-DOM-мутации ЧАСТИЧНО
UI-блокирующими вместо полностью асинхронных — компромисс «дешевле, но на
UI-потоке» вместо «дороже, но всегда off-thread». Нужен тот же протокол, что
и любая правка на этом файле по `docs/perf-method.md`: census ДО, интерливед
A/B (стоимость инкрементального прохода на UI-потоке per rAF-tick против
частоты пропущенных кадров) на представительном наборе сайтов с активным
rAF-циклом (ria.ru — готовый стенд), не только на синтетике. Масштаб и риск
измерения — сравним с BUG-341 (27 срезов), не точечный P3-фикс; следующая
сессия должна начать с `docs/perf-method.md` и живого замера на ria.ru по
методу этого файла.

## Воспроизведение

```
LUMEN_PROFILE_TREE=1 LUMEN_FRAME_LOG=1 cargo run --profile dev-release -p lumen-shell -- --mcp-live-port 18790 https://ria.ru
```

Подключиться MCP-клиентом (токен в stderr `[mcp] token: ...`), `wait
document_ready`, подождать ~2с, наблюдать `[engine] relayout Nms
(off-thread) dl=... styled=...` в логе даже без единого `scroll`-вызова —
уже на этапе settle. Любая реальная страница с активным
rAF+DOM-мутацией (видео-плеер, карусель, «живая лента») воспроизводит то же.
