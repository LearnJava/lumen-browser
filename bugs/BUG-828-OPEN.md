# BUG-828 — Web Audio рендерит тишину: `startRendering()` отдаёт пустой буфер, `ended` не стреляет, автоматизация `AudioParam` ни на что не влияет, а исключение в `oncomplete` шим глотает

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 20 — 11 TIMEOUT остатка, механизм `offline-audio-silent`)
**Область:** `crates/js/src/web_audio.rs:638` (`OfflineAudioContext.prototype.startRendering` — «Phase 0: immediately resolve with a silent buffer», плюс `try { self.oncomplete(evt) } catch(e) {}`), `crates/js/src/web_audio.rs:656` (`suspend`/`resume` — `Promise.resolve()`-заглушки), там же граф узлов (`AudioBufferSourceNode`/`OscillatorNode` без события `ended`, `AudioParam` без расписания)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Три независимые грани, все молчаливые.

**1. Рендер — тишина.** Граф из осциллятора и `GainNode`, работающий весь
рендер, даёт буфер, в котором нет ни одного ненулевого сэмпла:

```js
const ctx = new OfflineAudioContext(1, 4410, 44100);
const osc = ctx.createOscillator(); const gain = ctx.createGain();
osc.connect(gain); gain.connect(ctx.destination);
gain.gain.setValueAtTime(0.25, 0);
gain.gain.linearRampToValueAtTime(1.0, 0.05);
osc.start(0);
const rendered = await ctx.startRendering();   // длина верна, содержимое — нули
```

**2. `ended` не приходит.** Ни через `source.onended`, ни через
`addEventListener('ended', …)` — а именно на этом ожидании стоит
`audiosource-onended.html` и `audiobuffersource-ended.html`.

**3. Исключение в `oncomplete` исчезает.** И это то, что превращает FAIL в
TIMEOUT: `webaudio/resources/audioparam-testing.js::createAudioGraphAndTest`
делает всё сравнение из `context.oncomplete`, а шим зовёт этот обработчик
внутри `try { … } catch(e) {}`. Сравнение с тишиной падает — и не говорит
об этом никому, задача `Audit` не завершается, тест висит до таймаута.

## Прямое измерение

`tests/wpt/verify_preload_script_audio_gaps.py` (2026-08-22, коммит
`79f7df91a`, `--seconds 5`, все пробы живы — по 9 тиков):

| проба | ожидалось | получено |
|---|---|---|
| `audio-source-ended` | `source-onended`, `source-ended-listener`, `rendered length=44100` | только `rendered length=44100` |
| `audio-param-automation` | `rendered nonzero=4410/4410` | `gain-value=1`, `rendered nonzero=0/4410` |
| `audio-oncomplete-throws` | `oncomplete-ran`, затем брошенное наружу исключение | `oncomplete-ran length=128`, `render-resolved` — и ни `win-error`, ни отказа промиса |
| `audio-offline-suspend` | `offline-suspended currentTime=0.5`, потом `offline-rendered` | `offline-suspended currentTime=0 state=closed`, `offline-rendered length=44100` |
| `audio-context-state` (контроль) | `statechange` на каждый переход | ровно это — `suspended`/`running`/`closed`, все промисы разрешились |

Контроль важен: машина состояний `AudioContext` сделана и работает, поэтому
дефект не в «Web Audio нет», а в том, что нет самого рендера.
`gain-value=1` — отдельная мелочь той же природы: `setValueAtTime(0.25, 0)`
не влияет даже на `.value`, то есть расписания у `AudioParam` нет вовсе.

## Причина (локализована чтением кода)

`OfflineAudioContext.prototype.startRendering` (`web_audio.rs:638`) сам
описывает себя: «Phase 0: immediately resolve with a silent buffer». Он
создаёт `AudioBuffer` нужной длины, переводит состояние в `closed`, зовёт
`oncomplete` и разрешает промис — обхода графа не происходит, поэтому ни
источники, ни автоматизация параметров ни на что не влияют. Событие `ended`
не диспатчится ниоткуда: у узлов-источников нет ни таймера, ни точки в
рендере, где `stop`/конец буфера мог бы его породить.

Глотание исключения — там же: `if (typeof self.oncomplete === 'function')
{ try { self.oncomplete(evt); } catch(e) {} }` и такой же `try/catch` вокруг
каждого слушателя `complete`. Обработчик события обязан отдавать исключение
в обычный путь «report the exception» (то есть в window `error`), а не
терять его; сейчас его не видит даже [BUG-716](BUG-716-OPEN.md)-овский путь,
потому что до него дело не доходит.

`OfflineAudioContext.prototype.suspend(t)` (`web_audio.rs:656`) —
`Promise.resolve()` без аргумента: рендер не приостанавливается, `currentTime`
остаётся нулём, а состояние к моменту колбэка уже `closed`.

## Масштаб

Механизм `offline-audio-silent` забирает **11 id** остатка снимка WPT-RUN-5
(все непонятые TIMEOUT категории `webaudio/the-audio-api`), по всему снимку —
13, считая два, что раньше сидели в слабой стадии «что-то бросило». Состав
остатка: 5 `the-audioparam-interface/audioparam-*` (все на грани 3), 2
`the-audiobuffersourcenode-interface/*ended*` (грань 2), 2
`the-audiocontext-interface/*`, 1 `the-analysernode-interface/
test-analyser-resume-after-suspended` (грань `suspend`), 1
`the-mediastreamaudiosourcenode-interface`.

Оценка снизу: в снимке `webaudio` вендорена частично, а весь корпус этой
категории построен вокруг сравнения отрендеренного буфера с эталоном, то
есть при живом рендере значимая часть перешла бы из TIMEOUT в честный
PASS/FAIL. Вне WPT цена: любой сайт, который синтезирует звук (визуализаторы,
игры, редакторы), на Lumen получает молчание без единой ошибки.

## Направление починки (не предписание)

Разделить на три независимых шага, по убыванию отдачи:

1. **Не глотать исключение** в `oncomplete`/`complete` — однострочная правка
   с непропорциональной отдачей: пять `audioparam-*` тестов сразу
   превращаются из TIMEOUT в FAIL с внятным сообщением, а это ровно тот
   сигнал, по которому дальше чинить рендер.
2. **`ended`** для `AudioBufferSourceNode`/`OscillatorNode` — событие можно
   поставить в очередь по расписанию (`start`/`stop`/длина буфера) даже
   до появления настоящего рендера.
3. **Собственно рендер** offline-контекста: обход графа с расписанием
   `AudioParam` (`setValueAtTime`/`linearRampToValueAtTime`/…). Самый
   дорогой шаг и единственный, который делает тесты зелёными, а не честно
   красными.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
   --variant audio-source-ended --variant audio-param-automation
   --variant audio-oncomplete-throws` — `source-onended`, `nonzero>0`,
   исключение видно снаружи.
2. WPT: `run_report.py --all --root webaudio/the-audio-api --recursive` —
   категория перестаёт висеть (даже если часть тестов станет FAIL).
