# BUG-1044 — клик автоматизации мимо цели

Стенд отвечает на один вопрос: что канал автоматизации (`--mcp-live-port`,
инструмент `click`) отвечает, когда точка, в которую разрешилась цель, физически
попадает не в неё. До правки — `{"success": true}` и чужой обработчик, после —
отказ с диагностикой.

Три цели на одной странице (`index.html`):

| Селектор | Форма | Ожидаемый ответ |
|---|---|---|
| `#ok` | обычная кнопка под точкой | `success` |
| `#under` | накрыт `position:absolute`-накладкой `#over` | отказ `Element click intercepted` |
| `#empty` | инлайн без собственного бокса | отказ `Element not found` |

Журнал страницы `window.__HITS` пишет capture-слушатель на `document`, то есть
видно не только «сработал ли обработчик цели», но и кто получил событие вместо
неё.

## Как гонять

Нужно живое окно и http одного origin (клик проверяется hit-тестом реального
layout, headless-дамп сюда не годится):

```bash
python -m http.server 8763 --bind 127.0.0.1 --directory samples/bug1044-click-miss &
python samples/bug1044-click-miss/drive.py
```

`drive.py` поднимает `target/dev-release/lumen.exe --mcp-live-port 8903
--maximized`, ждёт `document_ready` и печатает по каждой цели её
`getBoundingClientRect()`, ответ `click` и в конце `__HITS`.

## Замер 2026-09-11 (P6, E2E-4 итерация 6)

До правки (`git stash` тех же исходников, тот же бинарь-профиль):

```
click #ok    -> {"success": true}
click #under -> {"success": true}          ← промах, отчитанный успехом
click #empty -> Element not found
__HITS       -> ["target=ok","ok","target=over","over"]   ← сработал #over
```

После:

```
click #ok    -> {"success": true}
click #under -> Element click intercepted: point (100, 139) hits <div>#over
                (node 27) instead of the target element (node 24)
click #empty -> Element not found
__HITS       -> ["target=ok","ok"]         ← чужой обработчик не разбужен
```
