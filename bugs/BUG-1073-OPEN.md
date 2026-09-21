# BUG-1073 — при одновременном старте нескольких окон `lumen` часть процессов получает `present=WHITE` на Vulkan и GL и падает паникой `wgpu: Invalid surface` вместо перехода на следующий бэкенд

**Статус:** OPEN
**Тип:** дефект реализованного кода — выбор wgpu-бэкенда (`[probe]`, BUG-274/275) отклоняет Vulkan и GL как `present=WHITE`, но следующий шаг (`Surface::configure`) паникует в `wgpu-26.0.1/src/backend/wgpu_core.rs:3526`, а не идёт дальше по списку или деградирует в CPU-путь.
**Заведён:** 2026-09-21 (P2, WPT-RUN-7 срез 44, `pointerevents`)
**Область:** не локализовано — проба и выбор бэкенда (`crates/engine/paint/src/backends/wgpu_backend.rs`, `crates/engine/paint/src/renderer.rs` — `Renderer::new`/`Surface::configure`); причина `present=WHITE` при параллельном старте не установлена.
**Владелец:** P1 (wgpu-бэкенд, BUG-274/275).

## Симптом

`run_report.py --check --root /pointerevents --recursive --processes 4` — wptrunner запускает **4 экземпляра** `lumen.exe --bidi-port …` подряд с интервалом ~50 мс. Лог четвёртой из пяти попыток среза (`.tmp/pe44t-check1-aborted.log`, не отслеживается):

```
1:14.94 pid:34156 [probe] Vulkan: present=ok    texture=ok adapter="NVIDIA GeForce GTX 1050" (2041 мс) — ПРИНЯТ
1:15.55 pid:23508 [probe] Vulkan: present=WHITE texture=ok adapter="NVIDIA GeForce GTX 1050" (2783 мс) — отклонён
1:15.68 pid:19644 [probe] Vulkan: present=WHITE texture=ok adapter="NVIDIA GeForce GTX 1050" (2764 мс) — отклонён
1:16.68 pid:23508 [probe] GL:     present=WHITE texture=n/a adapter="NVIDIA GeForce GTX 1050/PCIe/SSE2" (1128 мс) — отклонён
1:16.82 pid:19644 [probe] GL:     present=WHITE texture=n/a …                                                   — отклонён
1:17.17 pid:23508 thread 'main' panicked at …/wgpu-26.0.1/src/backend/wgpu_core.rs:3526:18:
        wgpu error: Validation Error — In Surface::configure — Invalid surface
1:17.46 pid:19644 thread 'main' panicked at … (то же)
```

Два из четырёх процессов умирают на старте. Дальше цепочка чисто инфраструктурная: `BiDi connect` → `ConnectionRefusedError [WinError 1225]`, релонч, `IO Completion Port failed to signal process shutdown`, три релонча подряд не поднимают `--bidi-port` (`RuntimeError: lumen --bidi-port did not print [bidi] token`) — и `TestRunnerManager` падает, обрывая **весь** прогон на 12–93 тестах из 258; `--check` даёт сотни ложных `REGRESSION … got MISSING`.

Хвост (`did not print [bidi] token`) — тот же, что у [BUG-1072](BUG-1072-OPEN.md), но **триггер другой**: там зависший тест, здесь падение при старте.

## Воспроизведение и частота

- Тот же бинарь (`dev-release`, `origin/main` от 2026-09-21), та же категория: `--update-expected` (12:15), `--check` №1 и №2 (по ~10 мин) — чисто, ни одного `present=WHITE`. Затем **четыре прогона подряд оборвались** (на 93, 93, 12 и 71 тесте из 258), пятый — чистый (258/258, 0 регрессий).
- Одиночный и тройной запуск `lumen.exe --bidi-port N` вручную (12 с, бэкенд выбран за 738–815 мс, `present=ok` во всех трёх) сбой не воспроизвёл. То есть нужна одновременная инициализация четырёх окон плюс что-то ещё; **что именно — не выяснено**.
- Гипотезы, не проверенные: (а) состояние рабочего стола (окна `lumen` накрыты/расфокусированы другими окнами пользователя — проба измеряет `present` по видимому окну); (б) конкуренция четырёх Vulkan-презентов на одной GTX 1050; (в) нагрузка от параллельной сборки другой сессии (в момент второго обрыва в системе шли 8 `rustc`, но четвёртый обрыв случился при 0 `rustc`, так что не единственная причина).

## Ожидание

Если ни один бэкенд не прошёл пробу `present`, процесс не паникует в `Surface::configure`, а либо берёт следующий кандидат (DX12 — в BUG-274 он третий в порядке перебора), либо честно деградирует и продолжает работу — `lumen --bidi-port` для WPT-прогона окно не обязан показывать.

## Что это значит для WPT-прогонов

Любой `run_report.py --processes 4` на этой машине может оборваться без единого настоящего результата, а `--check` в этом случае красный по 100–250 «регрессиям», которые регрессиями не являются. Признак: `check: N regression(s)` при `tests: K/258`, где K заметно меньше числа id, и в логе `present=WHITE` / `Invalid surface`. Порядок работы — перезапуск с нуля, а не разбор «регрессий» (`docs/tasks/p2-test-track.md#test-3-срез-44-2026-09-21`).
