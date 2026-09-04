# BUG-999 — `<iframe src>`, добавленный из скрипта, не даёт `load` вовремя: шесть `promise_test` виснут, а какой именно доедет — лотерея

**Статус:** OPEN
**Заведён:** 2026-09-05 (P2, WPT-RUN-7 срез 4 — генерация expectations baseline для `webidl`)
**Область:** не локализован. Единственная общая точка блокировки всех шести подтестов —
ожидание события `load` у программно вставленного `<iframe src>`
(`tests/wpt/webidl/ecmascript-binding/support/create-realm.js`); кандидаты — доставка
`load` для вложенного browsing context (shell/`lumen-js`) и создание самого контекста
**Владелец:** P3

## Симптом

`/webidl/ecmascript-binding/global-object-implicit-this-value-cross-realm.html` (6 подтестов)
**всегда** заканчивается `TEST_END: Test TIMEOUT`, но число доехавших подтестов от прогона к
прогону разное. Три изолированных прогона `run_smoke.py` подряд на одном и том же бинаре:

| прогон | подтестов прошло | что успело |
|---|---|---|
| 1 | 0/6 | ничего |
| 2 | 0/6 | ничего |
| 3 | 1/6 | `Cross-realm global object's getter called on null / undefined` — PASS |

Прогон категории целиком даёт третий вариант расклада: в `--update-expected` подтест
*getter* зафиксировался как `TIMEOUT`, а *setter* — как `NOTRUN`; в немедленно следующем
`--check` getter стал `PASS`, а setter — `TIMEOUT`. То есть один и тот же гейт видит
одновременно «регрессию» (`expected NOTRUN, got TIMEOUT`) и «неожиданный PASS» на двух
соседних подтестах одного файла.

## Почему это важно не как «ещё один падающий тест»

Тест падает по таймауту в любом случае — сам по себе это рядовой пробел. Значимо то, что
**подтестовый baseline такого теста невоспроизводим в принципе**: `expectations.py`
записывает статус каждого подтеста, а здесь статус определяется тем, кто успел до внешнего
дедлайна. Категория `webidl` из-за этого — единственная из 18 в срезе 4, для которой
обязательный порядок «`--update-expected`, сразу же `--check`, exit 0» не выполняется: гейт
краснеет на собственном, только что снятом baseline. Пока это так, baseline `webidl`
**не закоммичен** — иначе `--check` был бы красным у всех.

## Воспроизведение

```
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_smoke.py \
  --binary 'D:/RustProjects/lumen-browser/target/dev-release/lumen.exe' \
  /webidl/ecmascript-binding/global-object-implicit-this-value-cross-realm.html
```

(Git Bash: `MSYS_NO_PATHCONV=1`, иначе ведущий `/` съедается.) Читать строку
`TEST_END: Test TIMEOUT. Subtests passed N/6` — **N плавает, и это и есть дефект**; один
прогон ничего не доказывает, нужна серия из 3–5.

## Что известно о механизме

Все шесть подтестов — `promise_test`, и каждый первым делом делает `await createRealm(t)`.
`createRealm` (`support/create-realm.js`, вендоренный апстрим, не наша правка) — это

```js
const iframe = document.createElement("iframe");
iframe.onload = () => { resolve(iframe.contentWindow); };
iframe.src = "support/dummy-iframe.html";
document.body.append(iframe);
```

то есть промис резолвится **только** по `load` программно вставленного `<iframe src>`.
Ни один подтест не доходит до собственных ассертов, пока этот `load` не пришёл, поэтому
одна общая причина объясняет и полный таймаут, и разброс: подтесты соревнуются за успеть
до дедлайна harness'а, а не падают на разных ассертах.

**Это ещё не локализация.** Не проверено, приходит ли `load` вообще (просто поздно) или не
приходит никогда; не проверено, создаётся ли вложенный browsing context. Пробу нельзя
делать через `--dump-layout`/`--screenshot`: неинтерактивный путь исполняет скрипты один
раз при разборе и не докручивает события после — см. [`docs/engine-gaps.md`](../docs/engine-gaps.md)
и заметку об этом в разделе TEST-4 [`docs/tasks/p2-test-track.md`](../docs/tasks/p2-test-track.md).
Мерить нужно живым окном либо через BiDi.

## Смежное

- Пограничная область с [BUG-797](BUG-797-OPEN.md) (`window.open` без реального канала ко
  второй вкладке) — там тоже вложенный контекст, но другой вход и другой симптом (TIMEOUT
  без единого подтеста), поэтому заведено отдельно, а не дополнением.
- Категория `webidl` в остальном здорова: 56/62 harness OK, 281/614 подтестов — то есть это
  точечный дефект, а не «`webidl` не поддержан».
