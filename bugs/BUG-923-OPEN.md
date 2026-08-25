# BUG-923 — `Audio`/`Image`/`Option` — обычные JS-функции, а не legacy factory functions; и прототип элемента не равен `Interface.prototype` ни у одного тега

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, попутно к [BUG-799](BUG-799-FIXED.md))
**Область:** js (`crates/js/src/audio_element.rs:585-593` — `globalThis.Audio`; `crates/js/src/dom.rs` — `Image`/`Option`; `_lumen_build_element`/`_lumen_element_prototype_for` — цепочка прототипов обёртки)
**Владелец:** P1/P3

## Симптом

Замер (`--dump-layout` по http, `.tmp/aprobe/p5.html` и `p6.html`):

```
Audio.name=AudioConstructor            Audio.length=1
Audio.prototype === HTMLAudioElement.prototype   false
Audio()-no-new: NO THROW               HTMLAudioElement()-no-new: threw TypeError
Image()-no-new: NO THROW               Option()-no-new: NO THROW
new Audio().getAttribute('preload')    null      (IDL .preload отдаёт 'auto')
new Audio('x.mp3').getAttribute('src') x.mp3     (ок)

Object.getPrototypeOf(new Audio())  === HTMLAudioElement.prototype   false
Object.getPrototypeOf(createElement('div')) === HTMLDivElement.prototype  false
Object.getPrototypeOf(createElement('img')) === HTMLImageElement.prototype false
Object.getPrototypeOf(Object.getPrototypeOf(el)) === Interface.prototype  true  (все три)
```

Два независимых дефекта, попавшие в один тест.

### 1. Именованные конструкторы не конструкторы

`Audio` объявлен как

```js
globalThis.Audio = function AudioConstructor(src) {
  var el = document.createElement('audio');
  if (src !== undefined) el.src = String(src);
  return el;
};
```

— обычная функция, возвращающая объект. Отсюда сразу четыре расхождения с
HTML LS §4.8.11 / WebIDL «legacy factory function»: вызов **без `new`**
обязан бросать `TypeError` (не бросает), `.name` обязано быть `Audio`
(отдаёт `AudioConstructor`), `Audio.prototype` обязан быть тем же объектом,
что `HTMLAudioElement.prototype`, и конструктор обязан **выставить
контент-атрибут `preload="auto"`** (не выставляет; IDL-геттер отвечает
`auto` сам по себе, поэтому расхождение видно только через
`getAttribute`).

Ровно та же форма у `Image` и `Option` — обе зовутся без `new` без ошибки.
`HTMLAudioElement()` при этом `TypeError` бросает корректно, то есть
дефект именно в этих трёх обёртках, а не в общем механизме интерфейсов.

### 2. Прототип элемента — не `Interface.prototype`, и это про все теги

`Object.getPrototypeOf(el)` отдаёт per-interface прототип обёртки, который
стоит **на одно звено ниже** интерфейсного (та самая конструкция, которую
`CLAUDE.md` описывает в записи про [BUG-796](BUG-796-FIXED.md): члены из
`_LUMEN_WRAPPER_MEMBERS` ставятся на прототип-на-интерфейс, а он лежит под
`Interface.prototype`). Поэтому `el instanceof HTMLDivElement` — `true`
(цепочка на месте, [BUG-322](BUG-322-FIXED.md)), а прямое сравнение
прототипа — `false` **для любого элемента**, не только для `<audio>`.

## Почему это стоит больше, чем выглядит

`assert_equals(Object.getPrototypeOf(x), Interface.prototype)` — стандартный
способ, которым WPT проверяет фабрику объектов; сообщение при этом
бесполезно (`expected object "[object Object]" but got object "[object
Object]"`), потому что обе стороны сериализуются одинаково. Дефект №2 бьёт
по любой категории, где такой ассерт встречается, а не по аудио.

Дефект №1 виден и вне WPT: `Audio()`/`Image()` без `new` — приём старых
библиотек, и у нас он тихо «работает», то есть страница получает элемент
там, где реальный браузер бросил бы, — расхождение, которое проявится не на
этой строке, а позже.

## Наблюдаемое в WPT

`html/semantics/embedded-content/the-audio-element/audio_constructor.html`:
`Prototype of object created with named constructor` — FAIL (дефект №2),
`Calling Audio should throw` — FAIL (дефект №1). Шесть подтестов той же
страницы, где `preload="auto"` тоже проверяется, до него не доходят —
падают строкой выше на [BUG-922](BUG-922-OPEN.md), так что дефект №1 в
части `preload` наблюдаем только после его починки.

## Как проверить фикс

`Audio.name === 'Audio'`; `Audio()`, `Image()`, `Option()` без `new` бросают
`TypeError`; `Audio.prototype === HTMLAudioElement.prototype`;
`new Audio().getAttribute('preload') === 'auto'`;
`Object.getPrototypeOf(document.createElement('div')) === HTMLDivElement.prototype`
(и то же для `img`/`audio`) — при сохранении `instanceof` и всех членов
обёртки.
