---
name: graphic tests pipeline
description: Где искать workflow захвата скринов Lumen для graphic tests, ключевые пути и триггерные фразы — детали в коде, не дублировать здесь
type: reference
originSessionId: 1ec51f24-4dd6-4cf1-acd3-b44c187db707
---
## Single source of truth

Полный workflow — `graphic_tests/run.py` (Python). Запуск:

```bash
python graphic_tests/run.py            # блокирующий пайплайн
python graphic_tests/run.py --only 03  # только тест 03
python graphic_tests/run.py --continue-on-fail  # диагностика, не останавливается
```

Если что-то не работает — читай скрипт, а не эту память. В коде есть актуальные пути и пороги.

## Триггерные фразы

| Фраза | Действие |
|---|---|
| **«Ищи баги по скринам»** / **«Прогони graphic_tests»** | Запустить `python graphic_tests/run.py` |
| **«Ищи баги по скрину N»** | `python graphic_tests/run.py --only N` (N = два знака, например `03`) |

## Ключевые пути

- `D:/RustProjects/lumen-browser/utils/ffmpeg.exe` — ffmpeg (gdigrab build, в `.gitignore`)
- `C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe` — Edge headless
- `target/release/lumen.exe` — Lumen бинарь (`cargo build -p lumen-shell --release`, ~4 мин первый раз)
- `graphic_tests/screenshots/` — скрины (gitignored)
- `graphic_tests/BUGS.md` — реестр визуальных багов

## Маркер для crop offset

В каждой тест-странице (01–20) — `<div class="__m"></div>` первым ребёнком body, рендерится как 1024×1 магента-полоска (`#ff00ff`). Тест 00-calibration имеет верхнюю + нижнюю полоски — workflow находит их в desktop-снимке Lumen и определяет точные координаты content area. **Не используй жёсткое `crop=...:8:39`** — окно у winit может оказаться в любой точке десктопа.

## Артефакты Edge headless

Edge рисует серые scrollbar-полоски справа и снизу даже когда контент полностью в viewport. В diff с Lumen это пиксели расхождения, **не баг Lumen** — у Lumen scrollbar-UI в Phase 0 нет. На странице 01-sanity (бывшая 00) Lumen pixel-perfect — 0.00% diff после калибровки.

## Что НЕ дублировать в этой памяти

- Нумерацию тестов и описания (есть в `graphic_tests/index.html` + `COVERAGE.md`)
- Точные команды ffmpeg (есть в `graphic_tests/run.py`)
- Crop offset (определяется динамически, не fixed число)
- gdigrab title-баги (история, не актуально с момента перехода на `-i desktop`)

Если найдёшь, что workflow поменялся (новый формат, новый ffmpeg-флаг) — правь `graphic_tests/run.py`, не эту запись.
