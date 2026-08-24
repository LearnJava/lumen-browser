# BUG-828 — Web Audio рендерит тишину: `startRendering()` отдаёт пустой буфер, `ended` не стреляет, автоматизация `AudioParam` ни на что не влияет, а исключение в `oncomplete` шим глотает

**Статус:** FIXED 2026-08-25 (P1)
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

1. `python tests/wpt/verify_preload_script_audio_gaps.py
   --variant audio-source-ended --variant audio-param-automation
   --variant audio-oncomplete-throws` — `source-onended`, `nonzero>0`,
   исключение видно снаружи.
2. WPT: `run_report.py --all --root webaudio/the-audio-api --recursive` —
   категория перестаёт висеть (даже если часть тестов станет FAIL).

## Починено 2026-08-25 (P1)

Все три грани закрыты, плюс четвёртая (`suspend`). Правка целиком в JS-шиме
`crates/js/src/web_audio.rs` — рендер живёт там же, где граф, потому что
натива для него нет и переносить граф в Rust ради 128-сэмплового цикла нечего.

**1. Рендер.** `startRendering()` больше не отдаёт готовый нулевой буфер: он
запускает `_renderStep()` — pull-обход графа от `destination` квантами по 128
кадров. Каждый узел кэширует свой выход под номером кванта (`_qid`), поэтому
fan-out тянет источник ровно один раз, а **цикл в графе не уходит в
бесконечную рекурсию**: слот занимается тишиной *до* спуска в предков, так
что второй визит внутри одного кванта читает нули (тест
`bug828_feedback_cycle_terminates`). Реально синтезируют `OscillatorNode`
(sine/square/sawtooth/triangle, частота+detune по расписанию),
`AudioBufferSourceNode` (offset/duration/loop/playbackRate, линейная
интерполяция) и новый `ConstantSourceNode`; обрабатывают вход `GainNode`,
`DelayNode` (кольцевая линия), `BiquadFilterNode` (коэффициенты RBJ в
специализации §1.10 — `Q` в децибелах для low/highpass, k-rate на квант, плюс
настоящий `getFrequencyResponse`), `StereoPannerNode`, `WaveShaperNode`,
`ChannelMergerNode`/`ChannelSplitterNode`. `DynamicsCompressorNode`,
`PannerNode`, `ConvolverNode`, `AudioWorkletNode` остаются пропусканием
входа — это осознанный остаток, а не забытая ветка.

**2. Расписание `AudioParam`.** Событие складывается в список и компилируется
в отрезки (`_build`/`_segValue`, §1.6.3): `set`/`lin`/`exp`/`target`/`curve`,
с граничными правилами спеки (экспонента при нулевом или разнознаковом `V0`
держит `V0`; `T0`/`V0` рампы — время предыдущего события и значение в нём).
Значения снимаются a-rate по сэмплам (k-rate — раз на квант), и **узел,
подключённый в параметр** (`osc.connect(gain.gain)`), суммируется поверх
кривой. `cancelScheduledValues`/`cancelAndHoldAtTime` работают.

**3. `ended`.** У источников появилась общая часть `_wa_source`: `start`/`stop`
пишут времена, событие уходит **задачей** и ровно один раз — из рендера, когда
квант перешагнул `stopTime` или кончился буфер. Для живого `AudioContext`
рендера нет, поэтому там оно ставится по стенным часам от `currentTime`,
который теперь тоже идёт (квантованный `Date.now()`-дельта с паузой на
`suspended`/`closed`) — без движущегося времени планировать было бы нечего.

**4. `OfflineAudioContext.suspend(t)`** останавливает цикл на границе кванта
(`ceil(t*sr/128)*128`, как требует спека — «quantized and rounded up»),
переводит контекст в `suspended` и разрешает свой промис; `resume()`
продолжает с того же кадра. Замер: `suspend(0.5)` на 44100 Гц отдаёт
`currentTime = 22144/44100 = 0.5021315…`, а не 0.5 — это и есть округление
вверх до 173-го кванта, ожидание «ровно 0.5» в заявке было приблизительным.

**Форма фикса, которую стоит унести с собой.** Всё, что шим зовёт от имени
страницы — `ended`, `complete`, `statechange`, разрешение промиса suspend, —
идёт через `_wa_task`, то есть задачей, а не встык (урок BUG-808: WPT ставит
`EventWatcher` **после** вызова-триггера, и синхронная доставка проваливает
подтест как «Not expecting event»). В рантайме без DOM (`--dump-*`,
растеризация SVG, юнит-тесты этого модуля) `setTimeout` нет вовсе — там
`_wa_task` зовёт колбэк на месте, и это единственная конфигурация, где
разницу некому наблюдать; отсюда же то, что юнит-тесты читают отрендеренный
буфер из `oncomplete`, а не из `.then` — микрозадачи V8 сливает по концу
скрипта, поэтому промис в том же `eval` не сработает никогда, и двум тестам
пришлось разъехаться на два `eval`.

**Одно исключение из этого правила — `statechange`.** Его спека тоже требует
ставить задачей, и первый вариант фикса так и делал; замер контрольного
варианта пробы показал, что это **хуже**: движок качает таймеры только когда
перерисовывает, на статичной странице задача ждёт до секунды, и переход в
`running` доезжает уже после `close()` — обработчик читает `context.state` и
печатает `closed`. Машина состояний работала до BUG-828 и в него не входит,
поэтому диспатч оставлен синхронным с комментарием на месте.

**Побочно закрыто:** `AudioNode.disconnect` раньше не снимал ребро на стороне
приёмника (обратной стороны просто не было), `ChannelMergerNode`/
`ChannelSplitterNode` не различали индексы входов/выходов, а `connect(param)`
возвращал сам параметр вместо `undefined`.

**Остатки (не в этом баге):** нет FFT, поэтому `AnalyserNode.get*FrequencyData`
по-прежнему отдают пол шкалы (временна́я область — настоящая, из кольцевого
буфера); `decodeAudioData` по-прежнему отдаёт секунду тишины (декодера нет);
живой `AudioContext` по-прежнему не звучит — его никто не связывает с
устройством вывода.
