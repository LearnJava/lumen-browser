# BUG-925 — атрибут `loading` у медиа-элемента не существует: не рефлектится и ничего не откладывает

**Статус:** FIXED 2026-09-23 (P3)
**Заведён:** 2026-08-25 (P1, попутно к [BUG-799](BUG-799-FIXED.md))
**Область:** js (`crates/js/src/audio_element.rs` — `patchAudioElement`, ни `loading`, ни отложенной загрузки; `crates/js/src/video_bindings.rs` — то же у `<video>`)
**Владелец:** P1/P3

## Исправлено

Две независимые правки.

**Рефлексия.** `<img>`/`<iframe>` — строка в `_lumen_install_reflection` (теперь
`crates/js/src/shim/web_api_shim_tail_b.js`, не `dom.rs`: файл переехал между
заведением бага и починкой) переведена с `'string'` на `'enum'` с общим
`_LUMEN_LOADING = { def: 'eager', keys: ['lazy', 'eager'] }`, тем же приёмом,
что уже применяет `referrerPolicy`/`fetchPriority`. `<audio>`/`<video>` не
имели строки вовсе — добавлена как fallback-строка в общий блок
`[HTMLVideoElement.prototype, HTMLAudioElement.prototype]`, но реальную
работу делает собственный accessor `el.loading` в `patchAudioElement`/
`patchVideoElement`, нужный, чтобы setter мог форсировать возобновление
отложенной загрузки (см. ниже) — тот же приём, каким `el.src` уже
переопределён поверх общей таблицы.

**Отложенная загрузка.** У `<audio>`/`<video>` resource selection algorithm
целиком на JS-стороне (в отличие от `<img>`, где ленивую загрузку ведёт
шелл), поэтому `loading="lazy"` реализован без списка кандидатов от шелла —
обычным `IntersectionObserver` на самом элементе:

- `audio_element.rs`: `startLoad(url)` стал шлюзом — при `loading==='lazy'`
  регистрирует `_lumen_defer_lazy_media_load(el, cb)` (новый хелпер,
  `crates/js/src/shim/web_api_shim_tail_mc.js`, рядом с ленивыми
  изображениями) вместо немедленного вызова; сама загрузка переехала в
  `startLoadNow(url)`.
- `video_element.js`: та же схема на единственной точке входа
  `resourceSelection`/`resourceSelectionNow` — покрывает оба пути выбора
  ресурса (`src`-атрибут и `<source>`-кандидаты).
- `_lumen_media_is_rendered(el, requiresControls)` — гейт «being rendered»
  из HTML LS: `!el.isConnected` / `hidden` / `display:none` навсегда
  блокируют загрузку (наблюдение из `audio-loading-lazy-not-rendered.html`/
  `video-loading-lazy-not-rendered.html` подтвердило именно постоянный
  пропуск, не одноразовую проверку), плюс для `<audio>` — отсутствие
  `controls` (не проверяется у `<video>`: он визуален по умолчанию).
  Проверка выполняется ВНУТРИ колбэка пересечения, а не в момент
  регистрации — маркуп в тестах (`to-eager`/`in-viewport`/
  `autoplay-when-visible`) выставляет `src` раньше `controls`.
- Переключение `loading` прочь от `'lazy'` — через IDL-свойство, через
  `setAttribute`, и через `removeAttribute` (единственный путь, не идущий
  через свойство) — форсирует немедленный запуск через
  `_resumeLazyLoadNow()`, не дожидаясь пересечения.

**Не в объёме этого закрытия:** отложенность самой загрузки у `<img>`/
`<iframe>` (только рефлексия), поддержка `<source>` у `<audio>` (WPT
`audio-loading-lazy-source-deferred.html` падает по независимой причине —
`<audio>` вообще не читает `<source>`-детей, отдельный, более широкий
пробел) — сами оценки замера в исходном тексте бага ниже это уже отмечали.

Тесты: 4 новых V8-теста в `audio_element.rs` (enum-дефолт, «без controls
никогда не грузится», «с controls грузится по пересечению», «removeAttribute
резюмирует немедленно»), 3 в `video_bindings.rs::media_element` (та же
триада без controls-специфики — видео визуально по умолчанию), 2 в новом
`crates/js/src/dom/tests/v8_bug925_img_iframe_loading_enum.rs` (дефолт/
инвалидация для `<img>`/`<iframe>`). `cargo test -p lumen-js --features
v8-backend` — весь крейт зелёный, `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` чист. Живой WPT-прогон
(`audio-loading-lazy-*`/`video-loading-lazy-*`) не выполнялся в этой сессии
— видимость новой семантики подтверждена unit-тестами на реальном
`IntersectionObserver`-контракте (video) и на его заглушке, повторяющей тот
же контракт (audio), а не полным browser-стендом.

## Симптом

Замер в живом окне (`.tmp/aprobe/p7.html`, абсолютные URL — чтобы не мерить
[BUG-924](BUG-924-FIXED.md)), три элемента: обычный, `display:none` с
`loading="lazy"`, и `loading="lazy"` на 1000vh ниже вьюпорта.

```
loading-idl-reflected=lazy   attr=null
EVENTS=plain:loadstart, lazyhidden:loadstart, lazybelow:loadstart,
       plain:loadeddata,  lazyhidden:loadeddata,  lazybelow:loadeddata, …
plain rs=4  lazyhidden rs=4  lazybelow rs=4
```

Две половины дефекта.

**Не рефлектится.** `el.loading = 'lazy'` кладёт обычное JS-свойство:
`getAttribute('loading')` остаётся `null`. То есть IDL-атрибута нет вовсе,
чтение «работает» только потому, что читается тот же экспандо. Замер по
четырём тегам сразу (`.tmp/aprobe/p12.html`) показывает и границу, и
соседний дефект:

```
audio:  idl=lazy  attr=null     invalid-> idl=BOGUS attr=null
video:  idl=lazy  attr=null     invalid-> idl=BOGUS attr=null
img:    idl=lazy  attr=lazy     invalid-> idl=BOGUS attr=BOGUS
iframe: idl=lazy  attr=lazy     invalid-> idl=BOGUS attr=BOGUS
```

— у `<img>`/`<iframe>` строка рефлексии есть (`dom.rs:17149` и `:17239`),
но объявлена как `'string'`, а не `'enum'`, поэтому недопустимое значение
читается обратно дословно вместо `'eager'`; у `<audio>`/`<video>` строки
нет ни одной. Чинить обе половины одним заходом: enum-механизм в
`_lumen_install_reflection` уже есть (`referrerPolicy` строкой ниже им
пользуется).

**Ничего не откладывает.** Оба ленивых элемента отработали полный цикл
загрузки — `loadstart` → … → `loadeddata`, `readyState` 4 — наравне с
обычным. Ни `display:none`, ни отсутствие `controls`, ни положение далеко
за нижней границей вьюпорта загрузку не отменяют и не отсрочивают.

## Что требует спека

HTML LS §4.8.11: у медиа-элемента `loading` — рефлектируемый enum
(`lazy`/`eager`, missing/invalid → `eager`), и `lazy` откладывает
запуск алгоритма выбора ресурса до пересечения с вьюпортом. Отдельная
ветка того же раздела: элемент, который **не отрисовывается**
(`display:none`, `hidden`, `<audio>` без `controls`), при `loading=lazy` не
должен обращаться к ресурсу вообще.

## Наблюдаемое в WPT

`html/semantics/embedded-content/the-audio-element/`:

| id | сейчас | почему |
|---|---|---|
| `audio-loading-lazy-not-rendered.html` | FAIL | ждёт, что `loadstart`/`error` не придут; приходят |
| `audio-loading-lazy-in-scroller.html` | FAIL | ждёт загрузки после прокрутки в контейнере |
| `audio-loading-lazy-to-eager.html` | TIMEOUT | смена `loading` на `eager` ничего не запускает |
| `audio-loading-lazy-autoplay-when-visible.html` | TIMEOUT | то же плюс автовоспроизведение |

Три соседних id (`audio-loading-load-deferred`,
`…-load-preload-auto-deferred`, `…-load-preload-metadata-deferred`) сейчас
**PASS, но вхолостую**: они проверяют `readyState === HAVE_NOTHING` через
1 000 мс, а под `wptserve` ресурс не грузится вовсе из-за
[BUG-924](BUG-924-FIXED.md). После починки BUG-924 они покраснеют и станут
честным замером этого бага — планировать порядок с этим расчётом.

## Как проверить фикс

`el.loading = 'lazy'` даёт `getAttribute('loading') === 'lazy'`, а
`el.loading = 'ЧУШЬ'` читается обратно как `'eager'`; `<audio loading=lazy>`
с `display:none` не порождает ни одного запроса на сервере пробы;
below-viewport `<audio loading=lazy controls>` не грузится до прокрутки и
грузится после неё; смена `loading` на `eager` запускает загрузку немедленно.
То же самое у `<video>` — модель там своя ([BUG-825](BUG-825-FIXED.md)),
рефлексии нет так же (замерено выше). Отложенность самой загрузки у
`<img>`/`<iframe>` этой пробой **не измерялась** — проверена только
рефлексия атрибута.
