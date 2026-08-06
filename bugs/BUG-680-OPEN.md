# BUG-680 — `SpeechSynthesisEvent`/`SpeechSynthesisErrorEvent` constructors don't exist; dispatched events are plain object literals, not instances of either

**Статус:** OPEN
**Компонент:** js (`crates/js/src/speech.rs` — `SPEECH_SHIM`, `SpeechSynthesisUtterance.prototype._fire` at line 136, globals export block at lines 273-289)
**Найден:** P2, WPT-VENDOR-speech-api, 2026-08-06

## Симптом

Категория `speech-api` (`tests/wpt/speech-api/`, 27 файлов, 16 отобранных
id — 8 файлов с суффиксом `-manual` в имени исключены генератором id) —
вендорена и прогнана целиком (`run_report.py --all --root speech-api
--recursive`, 1:36) — **9/16 harness OK, 6/31 сабтестов**. Необычно высокий
для 🚫-категории (заявлена вне скоупа — «нет речевого движка»), потому что
`speech.rs` на деле реализует настоящий TTS-мост (SAPI/espeak/`say` в
зависимости от ОС) и полноценный JS-шим `SpeechSynthesis`/
`SpeechSynthesisUtterance` — см. собственный doc-комментарий модуля,
CAPABILITIES.md перечисляет «Web Speech TTS (OS)» как реализованную фичу.

Все 9 FAIL на `SpeechSynthesisErrorEvent-constructor.html` и все 6 FAIL на
`SpeechSynthesisEvent-constructor.html` — одна причина:

```
new SpeechSynthesisEvent("type", {...})
// ReferenceError: SpeechSynthesisEvent is not defined
new SpeechSynthesisErrorEvent("type", {...})
// ReferenceError: SpeechSynthesisErrorEvent is not defined
```

`SPEECH_SHIM` (`speech.rs:93-292`) экспортирует на `window`/`globalThis`
только `SpeechSynthesisUtterance`, `SpeechSynthesisVoice`, `speechSynthesis`,
`SpeechRecognition`, `webkitSpeechRecognition` (строки 277-289) — классов
`SpeechSynthesisEvent`/`SpeechSynthesisErrorEvent` в шиме нет вовсе, ни как
глобалов, ни как приватных функций. Тесты спеки требуют оба конструктора
(`SpeechSynthesisEvent` — обязательные `utterance`; `SpeechSynthesisErrorEvent`
— обязательные `utterance`+`error`, оба бросают `TypeError` без корректного
`eventInitDict`), поэтому падают все 15 из 15 сабтестов двух файлов.

Дополнительно — сами события, которые движок реально доставляет, тоже не
являются инстансами этих классов: `SpeechSynthesisUtterance.prototype._fire`
(строка 136) строит plain object literal (`{type, utterance, charIndex,
charLength, elapsedTime}`), а не `new SpeechSynthesisEvent(...)`/`new
SpeechSynthesisErrorEvent(...)`. `SpeechSynthesisEvent-properties.html`
(проверяет свойства реально доставленного события) и
`SpeechSynthesis-speak-events.html`/`SpeechSynthesis-speak-twice.html`
(проверяют порядок `start`/`end`) падают отдельно, но по другой,
уже задокументированной причине — testdriver `bless()`/`click()` вызывает
`elementDocument.contains(...)`, отсутствующему на любом узле
([BUG-574](BUG-574-OPEN.md)); дефект конструкторов маскируется этим более
ранним падением в двух из трёх файлов, но виден напрямую в
`*-constructor.html`.

Прочий сигнал в категории — реконфирмация уже открытых дефектов, новых
номеров не заводилось: `SpeechRecognition-detached-iframe.window.html`
(`Cannot read properties of null (reading 'SpeechRecognition')` —
[BUG-480](BUG-480-OPEN.md), `<iframe>` без отдельного browsing context,
`contentWindow` = `null`); все `.https.` id (`SpeechRecognition-*.https.html`,
`SpeechSynthesisUtterance-basics.https.html`, `idlharness.https.window.html`)
TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer`; три `ERROR`
(`SpeechRecognition-installOnDevice`, `SpeechSynthesisUtterance-basics`,
`idlharness`) — класс BUG-380 (переиспользуемый browsing context отдаёт
результаты предыдущего теста после проваленной навигации); `historical.html`
двумя FAIL сообщает, что `webkitSpeechRecognition` существует и что
`SpeechRecognitionEvent` не реализует `interpretation`/`emma` — это
намеренный выбор шима (doc-комментарий строка 7: «stub», плюс сохранение
`webkitSpeechRecognition` как алиаса ради совместимости со старым кодом
страниц, как это до сих пор делают реальные браузеры) — не заводится как
баг, так как поведение соответствует документированному дизайну, а не
случайный пробел.

## Причина

`SPEECH_SHIM` реализует события как ad-hoc объектные литералы вместо
настоящих подклассов `Event`/`Event`-совместимых классов, и никогда не
устанавливает `SpeechSynthesisEvent`/`SpeechSynthesisErrorEvent` как
конструируемые со страницы глобалы — модуль довёл до конца саму TTS-логику
(очередь, тайминг, платформенный мост), но не WebIDL-поверхность событий,
которые эта логика генерирует.

## Масштаб

15 сабтестов в двух файлах (`SpeechSynthesisEvent-constructor.html`,
`SpeechSynthesisErrorEvent-constructor.html`) падают на самом первом вызове
конструктора. Любой код страницы, использующий `event instanceof
SpeechSynthesisEvent` для различения источников событий (а не полагающийся
на `event.type`), получит `false` даже для событий, доставленных самим
`speechSynthesis`.

## Дальше

Fix scope: добавить в `SPEECH_SHIM` два конструктора
`SpeechSynthesisEvent(type, eventInitDict)`/`SpeechSynthesisErrorEvent(type,
eventInitDict)` — оба наследуют от `Event` (шаблон уже есть в `dom.rs` для
других `Event`-подклассов), `eventInitDict.utterance` обязателен
(`TypeError` без него), `SpeechSynthesisErrorEvent` дополнительно требует
`eventInitDict.error` из перечисленного набора значений (`no-speech`/
`audio-busy`/`audio-hardware`/`network`/`synthesis-unavailable`/
`synthesis-failed`/`language-unavailable`/`voice-unavailable`/`text-too-long`/
`invalid-argument`/`not-allowed`); переключить `SpeechSynthesisUtterance.
prototype._fire` (строка 136) на `new SpeechSynthesisEvent(...)` (обычные
события) / `new SpeechSynthesisErrorEvent(...)` (событие `error`) вместо
текущего plain-literal.
