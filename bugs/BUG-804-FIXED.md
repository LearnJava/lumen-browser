# BUG-804 — `load`/`error` не диспатчатся для `<script>`/`<link>`/`<style>`, вставленных ПАРСЕРОМ, и для `<style>` вообще ни при какой вставке

**Статус:** FIXED 2026-08-25 (срезы 1–4)
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 12 — `html/semantics/document-metadata/the-style-element`, `html/semantics/scripting-1/the-script-element`)
**Область:** `crates/js/src/dom.rs` — `_lumen_resource_track` (строка ~5712: белый список тегов `'script'|'link'`, `'style'` отсутствует) и вся машинерия `_lumen_resource_*`, доступная только элементам из `createElement`/`createElementNS`; `crates/shell/src/main.rs` — парсерный путь загрузки подресурсов (`Загружен скрипт:` / `Загружен CSS:` / `Пропуск скрипта…` / `Пропуск CSS…`), который грузит ресурс и НИЧЕГО не сообщает JS
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Тест ждёт `load` (или `error`) от `<script src>`, `<link rel=stylesheet>`
или `<style>` и висит до таймаута хёрнесса — вместо FAIL получается
TIMEOUT, потому что `testharness.js` не успевает опубликовать
`harness_status`. Живой прогон:

```
tests/wpt/run_report.py --all --root html/semantics/document-metadata/the-style-element --recursive
  → tests: 11/26 harness OK
  NOTRUN If the style is loaded successfully, the 'load' event must be fired
  NOTRUN If the style is loaded unsuccessfully, the 'error' event must be fired
  TIMEOUT /html/semantics/document-metadata/the-style-element/style_events.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style_load_async.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style-load-after-mutate.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style-error-01.html

tests/wpt/run_report.py --all --root html/semantics/scripting-1/the-script-element --recursive
  → tests: 298/434 harness OK, 233 TIMEOUT
  NOTRUN Test that the insertion point is defined in the load event of a parser-inserted script.
  TIMEOUT /html/semantics/scripting-1/the-script-element/script-onload-insertion-point.html
  NOTRUN no src, parser-inserted, has style sheets blocking scripts, script nesting level == 2
  TIMEOUT /html/semantics/scripting-1/the-script-element/load-error-events-3.html
```

## Воспроизведение (A/B на одной странице, живое окно)

Страница отдаётся по http (`python3 -m http.server`), браузер —
`lumen --mcp-live-port N http://127.0.0.1:PORT/p2.html`; каждый обработчик
печатает строку в консоль:

```html
<style  onload="log('parser style onload')"  onerror="log('parser style onerror')">#a{color:red}</style>
<link rel=stylesheet href="ext.css"      onload="log('parser link onload')"      onerror="log('parser link onerror')">
<link rel=stylesheet href="missing.css"  onload="log('parser link404 onload')"   onerror="log('parser link404 onerror')">
<script src="ext.js"          onload="log('parser script onload')"    onerror="log('parser script onerror')"></script>
<script src="missing-404.js"  onload="log('parser script404 onload')" onerror="log('parser script404 onerror')"></script>
<script>
  const st = document.createElement('style');  st.textContent = '#a{margin:2px}';
  st.onload = () => log('dynamic style onload');   document.head.appendChild(st);
  const s  = document.createElement('script'); s.src = 'ext2.js';
  s.onload  = () => log('dynamic script onload');  document.head.appendChild(s);
  const l  = document.createElement('link');   l.rel = 'stylesheet'; l.href = 'ext.css';
  l.onload  = () => log('dynamic link onload');    document.head.appendChild(l);
</script>
```

Вывод браузера — ровно два события из девяти:

```
Загружен скрипт: http://127.0.0.1:18777/ext.js
Пропуск скрипта  http://127.0.0.1:18777/missing-404.js: network error: HTTP 404
[JS] PROBE: external script executed          ← парсерный скрипт ВЫПОЛНИЛСЯ
[JS] PROBE: sync end
Пропуск CSS      http://127.0.0.1:18777/missing.css: network error: HTTP 404
[JS] PROBE: dynamic external executed
[JS] PROBE: dynamic link onload               ← createElement-путь работает
[JS] PROBE: dynamic script onload             ← createElement-путь работает
```

То есть:

| вставка | `<script src>` | `<link rel=stylesheet>` | `<style>` |
|---|---|---|---|
| парсером (разметка страницы) | ресурс грузится и исполняется, **события нет** — ни `load`, ни `error` на 404 | лист грузится и попадает в каскад, **события нет** | лист применяется, **события нет** |
| `createElement` + вставка | `load` есть (BUG-571) | `load` есть (BUG-722) | **события нет** |

Ресурс при этом реально загружается и применяется — молчит именно
уведомление страницы, поэтому дефект невидим для всего, кроме кода,
который на событие подписан.

## Причина (локализована чтением кода)

Машинерия событий `load`/`error` (`_lumen_resource_pending`,
`_lumen_resource_try_prepare`, `_lumen_resource_fire`) была написана
[BUG-571](bugs/BUG-571-FIXED.md) под `<script>` и обобщена
[BUG-722](bugs/BUG-722-FIXED.md) на `<link rel=stylesheet>`. Вход в неё
ровно один — `_lumen_resource_track(nid, tag)`, который вызывается ТОЛЬКО
из `document.createElement`/`createElementNS` и имеет жёсткий белый список
тегов:

```js
function _lumen_resource_track(nid, local) {
    var tag = String(local).toLowerCase();
    if (tag !== 'script' && tag !== 'link') return;   // ← 'style' не проходит
    ...
}
```

Отсюда обе половины дефекта:

1. **`<style>` не проходит белый список** — ни из парсера, ни из
   `createElement`. `style.onload` не сработает никогда, даже на пути,
   который для `<script>`/`<link>` уже работает. Это самая дешёвая часть
   фикса: у инлайнового `<style>` нет сетевой загрузки, задача сводится к
   «после того как лист разобран и попал в каскад — задачным хопом
   выстрелить `load`» (а для `@import`, который не разрешился, — `error`,
   как требует `style-error-01.html`).

2. **Парсерный путь не подключён к машинерии вообще.** Комментарий над
   `_lumen_resource_pending` объявляет это намеренным, ссылаясь на
   спецификационный флаг *already started*: «Deliberately NOT covered …
   scripts that came from the document parser». Флаг корректен по своему
   назначению — он запрещает ПОВТОРНЫЙ запуск скрипта при перемещении
   элемента по дереву. Но HTML LS §4.12.1 (\"execute the script block\",
   шаги с `fire an event named load at el` / `named error`) требует
   выстрелить событие и для парсерного скрипта тоже, а §4.6.7 — для
   парсерного `<link>`. Шелл эти ресурсы грузит своим путём (строки
   `Загружен скрипт:` / `Загружен CSS:` / `Пропуск скрипта…` /
   `Пропуск CSS…` в `crates/shell/src/main.rs`) и не даёт JS-стороне
   никакого сигнала об исходе.

## Почему это TIMEOUT, а не FAIL

Тот же класс, что [BUG-622](bugs/BUG-622-OPEN.md), [BUG-795](bugs/BUG-795-DUPLICATE.md)
и хелпер-404 из среза 7: тест регистрирует `async_test`/`promise_test`,
который резолвится только из обработчика события. Событие не приходит,
`harness_status` не публикуется, wptrunner убивает страницу по таймауту.
Каждый такой тест стоит ~9 с настенного времени против 0.05 с у
разрешившегося (WPT-RUN-5, срез 15).

## Охват

- **Остаток `unclassified` снимка WPT-RUN-5:** 27 id ловятся строгим
  маркером (событийный атрибут на `<style>`/`<link>`/`<script>` в разметке,
  либо `createElement('style')` с ожиданием `load`/`error` НА ТОЙ ЖЕ
  переменной). Крупнейшие: `html/semantics` 15,
  `content-security-policy/style-src` 3.
- **По корпусу:** 156 файлов-источников — `<script … onload/onerror=>` 90,
  `<link … onload/onerror=>` 47, `<style … onload/onerror=>` 12,
  `createElement('style')` с ожиданием события 9.

Числа — нижняя граница: маркер требует, чтобы ожидание было привязано к
той же переменной (иначе `check-layout-th.js`, создающий `<style>` для
подсветки ошибок и не ждущий его, забирал бы себе 40 тестов `css-grid`),
и не видит форм вида `document.querySelector('style').addEventListener('load', …)`.

## Не путать

- [BUG-571](bugs/BUG-571-FIXED.md) / [BUG-722](bugs/BUG-722-FIXED.md) —
  `createElement`-путь для `<script>`/`<link>`; **работает**, подтверждено
  A/B выше. Чинить их заново не нужно.
- [BUG-630](bugs/BUG-630-OPEN.md) (`<img>`), [BUG-798](bugs/BUG-798-OPEN.md)
  (`<embed>`/`<object>`) — тот же КЛАСС («элемент не сообщает об исходе
  загрузки»), но другие элементы и другой код; общего фикса с ними нет.

## Разметочный `<track>` — сюда же (2026-08-24)

[BUG-775](bugs/BUG-775-FIXED.md) (поглотил [BUG-795](bugs/BUG-795-DUPLICATE.md))
закрыл **скриптовую** половину `<track>`: элемент, созданный через
`createElement`, теперь фетчится, разбирается и диспатчит `load`/`error`.
Разметочный `<track>` остался ровно в форме этого бага и по той же причине:
его грузит Rust-обход `shell::tracks::load_video_tracks` **до** того, как
страница успевает повесить обработчик, и никакого сигнала JS-стороне не даёт.

Измерено 2026-08-24 (`run_report.py --all --root
html/semantics/embedded-content/media-elements/track/track-element
--recursive`): из 43 файлов `track-webvtt-*.html`, которые BUG-795 считал
висящими, 34 теперь проходят — они создают трек через
`track-helpers.js::check_cues_from_track`; висят ровно **9**, где `<track>`
написан в разметке (`track-webvtt-bom.html`, `-magic-header.html`,
`-newlines.html`, `-utf8.html`, `-no-timings.html`, `-timings-hour.html`,
`-timings-no-hours.html`, `-header-comment.html`,
`-align-text-line-position.html`).

Развилка для фикса нетривиальна и потому не сделана попутно: JS-сторона умеет
запустить модель и для разметочного трека (это дало бы второй, тёплый по кэшу
запрос — та же аппроксимация, что уже задокументирована для
`_lumen_link_prepare`), но тогда `video.textTracks` начнёт строиться из
JS-списка вместо снимка шелла для **каждой** существующей страницы с
разметочными треками. Нужно сперва решить, кто из двоих владеет списком.

**Развилка снята срезом 4 (см. ниже) в пользу JS-списка.**
- [BUG-459](bugs/BUG-459-OPEN.md) — URL внешнего `<script type=module>`;
  ортогонально, событий не касается.

## Перезамер 2026-08-22 (WPT-RUN-6, срез 20): третья форма маркера

Слепое пятно строгого маркера — ожидание, которое живёт **не в тексте
теста**. Всё семейство `html/dom/render-blocking` ждёт `load` элемента через
свой хелпер (`support/test-render-blocking.js`: `new LoadObserver(el)` →
`target.addEventListener('load', …)` → `promise_test(() =>
loadObserver.load)`), поэтому слова `load` в самом файле нет вообще и
маркер его не видел.

Обе половины перемерены живьём
(`tests/wpt/verify_preload_script_audio_gaps.py`, коммит `79f7df91a`).
Ресурс на пробном сервере намеренно держится секунду, так что слушатель
заведомо повешен до прихода ответа:

| проба | получено |
|---|---|
| `script-parsed-load-listener` | `found-parsed-script=yes`, `slow-script-ran=number` — скрипт **выполнился**, но ни `addEventListener('load')`, ни `onload` не сработали |
| `link-parsed-stylesheet-load` | `found-parsed-link=yes`; лист загружен (сервер видел запрос), событий нет ни в одной форме |
| `style-element-load` | `style-appended sheet=no`, `style-sheet-later=no` — `load` нет никогда, и `style.sheet` остаётся `null` (CSSOM, [BUG-471](BUG-471-OPEN.md)) |
| `script-dynamic-load` / `script-dynamic-404` (контроль) | `script-load` и `script-error` — созданный скриптом элемент событие даёт |

Маркер расширен третьей формой: `LoadObserver`/`test_render_blocking` плюс
хотя бы один молчащий элемент под наблюдением (парсерный `<script src>` /
`<link rel=stylesheet>` / `<style>`, либо созданный `<style>`). Даёт **+6 id**
остатка снимка WPT-RUN-5 (27 → 33 по маркеру; остальные три файла того же
каталога висят раньше, на [BUG-827](BUG-827-FIXED.md), и разобраны там).

Четвёртая, отдельно измеренная грань того же дефекта: `<script src="">`
(пустой URL) не даёт `error` вовсе — проба `script-src-empty` не напечатала
ничего, тогда как HTML LS §4.12.1 требует поставить `error` в очередь
задач. Это ломает `fetch-src/empty.html` и `empty-with-base.html`, которые
на этом событии и завершаются.

---

## Как чинилось

Четыре среза, по одному на элемент. Общий вывод, который стоит унести
отдельно от самого бага: **место правки решает не элемент, а то, кто знает
исход.** Для `<script src>` и `<link rel=stylesheet>` исход знает только шелл
(он уже сходил в сеть, а повторный фетч из JS не отличил бы «лист в каскаде»
от «байты пришли»), поэтому срезы 1–2 правили шелл. Для `<style>` (срез 3) и
`<track>` (срез 4) сети либо нет вовсе, либо элемент и его событие
принадлежат DOM целиком — правка ушла в шим.

| срез | элемент | где правка | замер |
|---|---|---|---|
| 1 | парсерный `<script src>` | шелл (`ResolvedScript::external_ok`) | проба: 6 вариантов из 6 |
| 2 | парсерный `<link rel=stylesheet>` | шелл (`load_linked_stylesheets` → исход по узлу) | проба: 3 варианта из 3 |
| 3 | `<style>` на любом пути | шим (`_lumen_style_update_block`) | `the-style-element` 11/26 → **15/26** |
| 4 | разметочный `<track>` | шим (`_lumen_track_elements_scan`) | `track-element` 83/143 → **104/143** |

## Срез 4 — разметочный `<track>` (2026-08-25)

### Развилка владения списком снята в пользу JS

Причина не в том, что JS-список «лучше», а в том, что снимок шелла на этот
вопрос ответить **не может**: `PageTracks` ключуется по `<video>` и не несёт
идентичности `<track>` вообще (`TrackInfo` — это `kind`/`src`/`srclang`/
`label`/`default`, без `NodeId`), поэтому `trackElement.track` физически не
может оказаться тем же объектом, что `video.textTracks[i]`. А именно это
читают внутри обработчика все девять висевших тестов
(`var cues = track.track.cues;`). Три довода помельче, все в ту же сторону:

* JS-список спецификационно точнее там, где они расходятся — перечислимый
  `kind` с правилами «отсутствует → subtitles, невалиден → metadata», `mode`
  из атрибута `default` вместо эвристики «первый subtitles/captions», и
  `<audio>` как медиаэлемент (обход шелла `collect_video_tracks` смотрит
  только на `<video>`);
* для трека из `createElement` JS-список победил ещё в
  [BUG-775](BUG-775-FIXED.md), так что оставить разметку на снимке значило бы
  «у одной страницы два владельца, в зависимости от того, как каждый трек в
  неё попал»;
* **отрисовка не переезжает**: у шелла остаётся свой обход и своё хранилище
  cues, оверлей не меняется.

Цена — второй запрос файла (шелл для оверлея, шим для событий и `cues`).
Это та же аппроксимация, что уже несут `_lumen_link_prepare` и `@import` из
среза 3, и её уже платил путь `createElement`: проба показывает
`b804-cues.vtt?created x2` **до** этой правки.

### Пять дефектов, которых заявка не называла

Первый нашла проба «до», два следующих — юнит-тесты, написанные под срез,
четвёртый — проба «после», а пятый (и самый крупный) не нашёл бы никто, кроме
прогона **всей категории**: он ломает тест ещё до того, как хоть что-то
загрузится.

1. **`<audio><track></audio>` не грузился вообще.** Проба:
   `track-parsed-audio` → `server saw: nothing`, тогда как у `<video>` запрос
   был. §4.8.11.1 шаг 3 говорит «медиаэлемент», а `collect_video_tracks`
   ищет `video`.
2. **`textTracks` не существовало на `<audio>`.** Это own-свойство, которое
   `patchVideoElement` вешает на каждую обёртку `<video>`; у `<audio>` своя
   модель (`audio_element.rs`), через этот патч она не проходит. Трек
   загружался и попадать ему было некуда. Аксессор переехал на
   `HTMLMediaElement.prototype` — own-свойство `<video>` его по-прежнему
   затеняет, так что путь `<video>` не тронут.
3. **Список перестал быть в порядке дерева.** Комментарий в
   `startTrackLoad` утверждал «список в порядке дерева», и это было верно,
   пока вход был один: скрипт вставляет треки по порядку. Проход по разметке
   идёт **после** скриптов документа, поэтому трек, который головной скрипт
   дописал к разметочному `<video>`, вставал бы перед написанным в разметке.
   Позиция теперь считается по детям медиаэлемента, а не по порядку запуска
   модели.

Четвёртый, замеченный пробой уже после правки: событие приходило
`isTrusted: false` — ровно тот дефект «объекта события», который
[BUG-838](BUG-838-FIXED.md) чинил у `<script>`/`<link>`.

**Пятый: `video.textTracks` пуст в момент, когда скрипт документа вешает
обработчики.** После прохода из `interactive` шесть из девяти названных багом
тестов позеленели, а `track-webvtt-utf8`/`-timings-hour`/`-header-comment`
остались TIMEOUT — и не потому, что что-то не загрузилось. Они открываются так:

```js
for (var i = 0; i < video.textTracks.length; i++)
    trackElements[i].onload = t.step_func(trackLoaded);
```

Список строился лениво из `_lumen_track_media_lists`, куда трек попадал только
при **старте загрузки**, то есть на проходе `interactive` — уже после скриптов
документа. Значит `length` читался нулём и обработчиков не вешалось **ни
одного**; сколько бы правильно события потом ни приходили, тест обязан был
висеть. Между тем §4.8.11.1 добавляет текстовый трек в список, когда вставлен
**элемент**, задолго до прихода файла, — а для разметки вставка уже произошла к
моменту, когда шим вообще запускается. Отсюда `listMarkupTracks`: на установке
шима (там же, где уже шёл обход `document.querySelectorAll('video')`) `<track>`-дети
каждого медиаэлемента получают свои `TextTrack` и место в списке, а загрузка
по-прежнему стартует позже. Все три теста стали `OK 1/1`.

Это же и ответ на вопрос, зачем гонять категорию целиком, а не девять
названных id: дефект виден только там, где ожидание висит на **другом** API,
чем тот, который чинишь.

### Замеры

Проба `verify_bug804_parser_resource_events.py`, шесть новых вариантов
(`track-parsed-attr-load`, `-404`, `-listener-load`, `-audio`, `-no-src`,
плюс контроль `track-created-load`). До правки молчали четыре из пяти
подопытных, пятый печатал только `found-parsed-track=yes`; контроль работал.
После — все шесть дают ожидаемый маркер, включая
`parsed-track-load cues=1 same=true ready=2` (то есть `track.track` — это
`video.textTracks[0]`, и cues из него читаются).

Категория `html/semantics/embedded-content/media-elements/track/track-element`
целиком: **83/143 → 104/143 harness OK**, подтесты **15/168 → 29/168**,
регрессий **ноль** (по-id A/B обоих прогонов). 21 тест TIMEOUT → OK, включая
все девять `track-webvtt-*`, названные заявкой; `track-api-texttracks.html`
2/3 → 3/3.

13 юнит-тестов в `track_loading` (9 старых + 4 новых).

## Остаток

Не входит в этот баг, но обнаружено или подтверждено при его закрытии:

* **двойной фетч** файла `.vtt` для разметочного трека под `<video>` (шелл —
  для оверлея, шим — для событий); у `<audio>` запрос ровно один, потому что
  шелл туда не ходит;
* **cues, загруженные шимом, не рисуются** — оверлей берёт своё хранилище от
  обхода шелла (остаток [BUG-775](BUG-775-FIXED.md));
* провал **связывания** модуля даёт `load` вместо `error` (остаток среза 1:
  `ModuleFailure` схлопывается в `JsResult`);
* `<link media=print>` на экране не грузится вовсе, поэтому не даёт и события
  (остаток среза 2);
* `style.sheet` по-прежнему `null` — CSSOM, [BUG-471](BUG-471-OPEN.md).
