# BUG-955 — `<audio>`/`<video>` не бросают `loadstart` при `src=""`, `<audio>` не бросает вообще ничего

**Статус:** OPEN
**Тип:** дефект реализованного кода — алгоритм выбора ресурса (HTML LS §4.8.11.5) реализован и в целом верен (см. `assigning_src_runs_resource_selection_and_reports_the_failure` в `video_bindings.rs`); не хватает одного шага для одного конкретного случая входа (`src=""`).
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 33, живая проба `verify_slice33_gaps.py --variant media-empty-src-loadstart`)
**Область:** js (`crates/js/src/video_bindings.rs::startFetch` — ранний `return` перед `queueEvent('loadstart')`; `crates/js/src/audio_element.rs::startLoad` — `if (!HAS_PROVIDER || !url) return;` не срабатывает совсем)
**Владелец:** P3.

## Симптом

`html/semantics/embedded-content/media-elements/location-of-the-media-resource/currentSrc.html`
создаёт `<audio>`/`<video>` скриптом, присваивает `.src = ''` и ждёт события
`loadstart`, внутри обработчика которого делает проверки и зовёт `t.done()`.
Для `src=""` `loadstart` не приходит НИ на одном из двух тегов — 4 из 16
асинхронных тестов файла (`'audio'`/`'video'` × `src=''`-кейс × 2 формы,
прямой `src` и `<source>`) никогда не вызывают `done()`, из-за чего харнесс
не завершается вовсе (`add_completion_callback` ждёт все тесты) — файл
целиком уходит в TIMEOUT вместо частичного FAIL/PASS.

## Причина

Две независимые реализации (audio/video — разные шимы, см. готчу CLAUDE.md
про «per-feature shim, тот же дефект чинится дважды»), два разных бага одной
формы:

- **`<video>`** (`video_bindings.rs::startFetch`): 
  ```js
  function startFetch(gen, url, candidate) {
    var abs = (url === '') ? null : resolveUrl(url);
    if (abs === null) { failResource(gen, candidate, 'unresolvable URL'); return; }
    _currentSrc = abs;
    _networkState = NETWORK_LOADING;
    queueEvent('loadstart');
    …
  }
  ```
  Для `url === ''` `abs` сразу `null`, и функция возвращает через
  `failResource(...)` ДО строки `queueEvent('loadstart')` — событие просто не
  успевает встать в очередь. `error` при этом приходит (через
  `failResource`), так что `<video src="">` даёт `error` без `loadstart`.

- **`<audio>`** (`audio_element.rs::startLoad`):
  ```js
  function startLoad(url) {
    if (!HAS_PROVIDER || !url) return;
    …
    fireEvent(el, 'loadstart');
    …
  }
  ```
  и в сеттере `src`: `if (_src) startLoad(_src);` — обе проверки берут `url`
  как булево значение, так что `src=""` (пустая строка, falsy) не доходит до
  `startLoad` вовсе. `<audio src="">` не даёт НИ `loadstart`, НИ `error` —
  тише, чем `<video>`.

По HTML LS §4.8.11.5 «resource selection algorithm» шаг с `loadstart`
относится к самому факту присутствия атрибута `src` (шаг «If mode is
attribute … queue a task to fire loadstart») и не зависит от того, резолвится
ли URL — `loadstart` обязан прийти даже для заведомо непригодного значения,
после чего уже отдельным шагом идёт `error`/«dedicated media source failure
steps». Обе реализации завязали `loadstart` на успешность резолва/непустоту
строки вместо самого факта попытки загрузки.

## Прямое измерение

Живая проба (`--variant media-empty-src-loadstart`, dev-release,
`main` = `8b634befc`): создать `<video>`/`<audio>`, повесить слушатели
`loadstart`/`error`, присвоить `src=""`, вставить в документ, подождать 3 с.
Маркеры за это время: `video-error` (пришёл), `media-empty-src-done` (таймер
дождался). `video-loadstart` и `audio-loadstart` — ни один не напечатан;
`audio-error` — тоже не напечатан (подтверждает: `<audio>` вообще ничего не
делает на `src=""`, ни одно из двух событий).

## Кого это держит

`currentSrc.html` — 1 id (полностью, TIMEOUT из-за 4 из 16 подтестов).
Возможно есть соседние файлы той же директории с тем же паттерном
(`src=""`/`<source src="">` на `<audio>`/`<video>`) — не проверялось отдельно
в рамках этого среза.

## Направление починки

`video_bindings.rs::startFetch`: звать `queueEvent('loadstart')`
безусловно, ДО ветки `abs === null`, а не после неё — `_networkState`
переходит в `NETWORK_LOADING` в любом случае перед выяснением, резолвится ли
URL. `audio_element.rs::startLoad`/сеттер `src`: убрать `|| !url` из
условия раннего выхода — пустая строка всё ещё «присутствующий src
attribute» по спеке и обязана пройти весь путь `loadstart` → `error`, как у
`<video>` (после починки первой половины — иначе получится два разных
неверных поведения вместо одного правильного).
