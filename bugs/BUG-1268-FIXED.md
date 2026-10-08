# BUG-1268 — тест, открывающий `blob:`-навигацию, роняет навигацию СЛЕДУЮЩЕГО теста в том же браузере

**Статус:** FIXED 2026-10-08
**Заведён:** 2026-10-04 (P2, найден при A/B `run_corpus.py --shared-queue`)
**Область:** shell / bidi-server (навигация верхнего контекста после `blob:`/`window.open`) — не локализовано

## Симптом

WPT гоняет тесты одного процесса `lumen` подряд в одном browsing context
(`executorlumen.py`). После двух тестов `FileAPI` следующий тест — любой,
независимо от содержимого — получает ERROR ещё до своей загрузки:

| предыдущий тест | что получает следующий |
|---|---|
| `/FileAPI/url/url-in-tags-revoke.window.html` | `browsingContext.navigate(...) failed: navigation failed: network error: unsupported scheme: blob` |
| `/FileAPI/BlobURL/cross-partition.https.html` | `navigate` отвечает успехом, но документ не заменён (`still at https://localhost:18443/FileAPI/BlobURL/cross-partition.https.html`) |

Сам `url-in-tags-revoke` тоже кончается ERROR (`unsupported scheme: about`):
тест делает `window.open()` + `win.location = blob:…` и
`frame.contentWindow.location = blob:…`.

## Как воспроизвести

```
<venv>/python tests/wpt/run_smoke.py --binary target/dev-release/lumen.exe \
    --processes=1 --no-manifest-update \
    /FileAPI/url/url-in-tags-revoke.window.html /dom/nodes/Element-hasAttribute.html
```

`Element-hasAttribute.html` один — `OK 2/2`; после `url-in-tags-revoke` —
`ERROR … unsupported scheme: blob`. То же с `cross-partition.https.html`
первым (`never replaced`). С `/FileAPI/url/url-in-tags.window.html` первым
отравления нет.

## Почему это важно

Вердикт чужого теста зависит от того, кто шёл перед ним в том же процессе.
В корпусном прогоне жертва случайна: при хеш-раскладке wptrunner ею были
`FileAPI/url/url-in-tags.window.html`, `console/console-count-logging.html`,
`FileAPI/blob/Blob-constructor-detached-buffer.any.html`; при
`--shared-queue` — `performance-timeline/navigation-id-element-timing`,
`…/performance-navigation-timing-not-bfcached` (замер
`docs/tasks/p2-wpt-runner-throughput.md` §общая очередь). Порядок тестов
меняется с любым изменением раннера, поэтому на каждом таком изменении
2–4 id «переезжают» из OK в ERROR и обратно, и это читается как регрессия.

## Чего не знаем

- Остаётся ли после `url-in-tags-revoke` в очереди отложенная навигация
  на `blob:` (из `win.location = blob:` или iframe), которую исполняет
  следующий `browsingContext.navigate`, или это ошибка в выборе контекста
  (`window.open` создал второй top-level, и `navigate` попал не туда).
- Почему после `cross-partition.https.html` навигация «успешна», но документ
  прежний.

Обход на стороне раннера возможен (перезапуск браузера после ERROR
навигации), но он спрячет дефект и удорожит прогон — не сделан.

## Корень и исправление (2026-10-08, P3)

Не отложенная навигация и не выбор контекста: страница теста во время
`browsingContext.navigate` открывает попапы (`window.open(blob:…)`,
`window.open()`), попап забирает передний план, и ожидание `DocumentReady`
читает `load_failed` попапа — `unsupported scheme: blob` / `about`. Попапы
старого документа, не успевшие открыться, всплывали тем же путём в
навигации СЛЕДУЮЩЕГО теста.

- `about_to_wait.rs`: пока идёт навигация автоматизации
  (`automation_tab` задан и `nav_start` выставлен), запросы `window.open`
  не вычитываются из рантайма — запросы старого документа умирают вместе
  с ним, запросы нового открываются после загрузки.
- `window.open()` / `location = 'about:blank'` больше не уходят в сеть:
  `PageSource::AboutBlank` (`resolve_js_navigation`).

Проверено `run_smoke.py --processes=1`: после `url-in-tags-revoke` и
`cross-partition.https.html` `Element-hasAttribute.html` — OK 2/2.
Сама загрузка `blob:` в iframe/попапе по-прежнему не поддержана (функция,
не баг): эти тесты TIMEOUT/ERROR, но соседей не роняют.
