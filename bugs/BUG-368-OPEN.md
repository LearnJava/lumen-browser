# BUG-368 — `innerHTML` на живом DOM — текстовая заглушка «Phase 0» на обоих движках: геттер отдаёт `textContent`, сеттер кладёт разметку одним текстовым узлом

**Статус:** OPEN
**Компонент:** js — нативные привязки `_lumen_get_inner_html`/`_lumen_set_inner_html`: `crates/js/src/v8_runtime.rs:1386-1407` (дефолтный движок V8) и `crates/js/src/dom.rs:878-899` (rquickjs); потребители — `dom.rs:5546-5547` (живой `Element`), `4255-4256`, `4322-4323`, `6307`/`6313`
**Найден:** P2, WPT-VENDOR-fenced-frame (2026-07-28), проба `--dump-layout` вне WPT

## Симптом

Обе половины `innerHTML` — заглушки, и обе задокументированы в коде как «Phase 0»,
хотя проект давно в Фазе 3 (v0.5). Ни `BUGS.md`, ни `CAPABILITIES.md` этого не
отражают: `CAPABILITIES.md:137` перечисляет `innerHTML` в строке «**DOM** — ✅ full
read/write», `subsystems/js.md:280` — в списке «DOM write».

Проба (`.tmp/ff-probe4.html`, дефолтный V8; разметка
`<div id=host><span id="a" class="c">A</span><!--k--><b>B</b>tail</div>`):

```
host.innerHTML            = "ABtail"          ← ожидается '<span id="a" class="c">A</span><!--k--><b>B</b>tail'
host.textContent          = "ABtail"          ← совпадает с innerHTML дословно
host.outerHTML            = undefined         ← отдельный, уже заведённый BUG-351
host.childNodes.length    = 4                 ← дерево при этом правильное
host.children.length      = 2
host.firstElementChild.tagName = SPAN
document.body.innerHTML   = "\nABtail\n\nconst R=(k,f)=>{try{console.log(…"   ← в выдачу попал исходник <script>

d = document.createElement("div"); d.innerHTML = '<i id="x">y</i>';
d.innerHTML               = "<i id=\"x\">y</i>"   ← round-trip строки создаёт иллюзию работы
d.childNodes.length       = 1
d.firstChild.tagName      = #text                ← разметка легла одним ТЕКСТОВЫМ узлом, элемента <i> нет
```

Последняя пара строк — главная ловушка: если проверять только «записал и
прочитал», API выглядит рабочим. Дефект виден лишь при взгляде на дерево
(`childNodes`) или на рендер — заданная разметка не парсится, а показывается
как текст.

## Причина

Прямо в теле привязок, дословно (обе реализации идентичны):

```rust
// crates/js/src/v8_runtime.rs:1386
reg!("_lumen_get_inner_html", move |node_id: u32| -> String {
    // Phase 0: return text content only (no HTML serialization).
    …
    collect_text_content(&doc, nid)
});
reg!("_lumen_set_inner_html", move |node_id: u32, html: String| {
    // Phase 0: treat innerHTML as plain text (no fragment parsing).
    …
    set_text_content(&mut doc, nid, &html);
});
```

То есть геттер — это буквально `textContent` (отсюда потеря тегов, атрибутов и
комментариев и попадание в выдачу текста `<script>`), а сеттер — `set_text_content`
(отсюда единственный текстовый узел вместо распарсенного фрагмента). Ни
сериализатора, ни fragment parsing нет ни на одном движке. Шим над сеттером
(`dom.rs:9791-9797`) только доставляет `MutationObserver`-уведомление и на
семантику не влияет.

При этом всё, чтобы это починить, в дереве уже есть: `lumen_html_parser::parse`
используется, например, шеллом для Document PiP, а обратная сериализация — в
`dom_parser.rs` (`VElement.prototype.outerHTML`, `dom_parser.rs:111`), но на
живое дерево ни то, ни другое не подключено.

## Масштаб

- **Шире WPT.** `innerHTML =` с разметкой — одна из самых частых операций
  реального веба (рендер шаблонов, вставка фрагментов, очистка через
  `el.innerHTML = ''` — последнее, кстати, работает: пустая строка эквивалентна
  очистке текста). Любая страница, строящая часть UI строкой HTML, получает
  видимый текст разметки вместо элементов.
- **Задевает собственную фичу браузера.** Document PiP (`subsystems/shell.md:637`)
  устроен как «сериализовать `innerHTML` контейнера → передать нативу →
  распарсить `lumen_html_parser::parse`». С текстовым геттером через этот канал
  проходит только текст — то есть перенос DOM-содержимого в PiP-окно теряет всю
  структуру.
- **Задевает уже заведённый баг.** Разбор [BUG-351](BUG-351-OPEN.md) (нет
  `outerHTML`/`insertAdjacentHTML`) исходил из того, что `innerHTML` рабочий, и
  предлагал чинить `outerHTML` рядом с ним. Чинить нужно вместе: сериализатор
  один и тот же.
- **Ложноположительные проверки.** Любой тест вида «`el.innerHTML = s;
  assert_equals(el.innerHTML, s)`» здесь зелёный, ничего не проверяя, —
  тот же класс ложноположительного, что зафиксирован в
  [BUG-362](BUG-362-OPEN.md)/`eventsource-url.any.html` и в `eye_dropper::tests`
  ([BUG-365](BUG-365-OPEN.md)). Верифицировать фикс нужно по `childNodes`/
  `children`, а не по round-trip строки.

## Что при этом корректно и ломать при фиксе не надо

- Само дерево, построенное HTML-парсером при загрузке страницы, полное и
  правильное (`childNodes.length = 4`, комментарий на месте, `children = 2`) —
  дефект только в двух привязках, не в DOM.
- `el.innerHTML = ''` как способ очистки работает и, скорее всего, используется
  в существующем коде/тестах.
- `MutationObserver`-обёртка сеттера (`dom.rs:9791`) корректна и должна пережить
  фикс.
- У «виртуального» дерева DOMParser (`dom_parser.rs`) сериализация своя и
  настоящая — её можно взять за основу, но нельзя просто переиспользовать: там
  другое представление узла (см. [BUG-358](BUG-358-OPEN.md) про раскол живого и
  виртуального дерева).

## Возможный фикс (не реализован в этой сессии)

1. Геттер: настоящая HTML-сериализация поддерева на нативной стороне
   (`lumen_dom` знает теги, атрибуты, текст и комментарии) — по HTML LS
   «fragment serializing algorithm», с экранированием `&`/`<`/`>` в тексте и
   `&`/`"` в значениях атрибутов и списком void-элементов.
2. Сеттер: `lumen_html_parser::parse` в режиме фрагмента с контекстным
   элементом, затем замена детей узла результатом (текущий `set_text_content`
   уже делает «replace all children», нужен тот же путь, но с распарсенным
   поддеревом).
3. Обе половины — на обоих движках (`v8_runtime.rs` и `dom.rs`), как это сделано
   для `_lumen_create_comment`/`_lumen_is_comment_node` в
   [BUG-326](BUG-326-FIXED.md).
4. `outerHTML` из [BUG-351](BUG-351-OPEN.md) — тот же сериализатор плюс сам узел;
   имеет смысл делать одним срезом.
5. Тест-верификация — только по дереву: `el.innerHTML = '<i>y</i>'` →
   `el.children.length === 1 && el.firstElementChild.tagName === 'I'`, плюс
   графический тест на видимый рендер вставленной разметки.

## Срез 26 (`css/css-ruby`, 2026-08-03) — первое прямое попадание реального WPT-теста

`position-relative.html` строит ruby-разметку через
`container.innerHTML = '<ruby style="position:relative">base<rt>annotation</ruby>'`,
затем читает `document.querySelector('rt').getBoundingClientRect()` — падает
с `Cannot read properties of undefined (reading 'getBoundingClientRect')`,
потому что `querySelector('rt')` возвращает `null`: подтверждено живой
пробой (`--mcp-port`) — `container.innerHTML = '<ruby>base<rt>annotation</ruby>'`
не создаёт вовсе НИ ОДНОГО элемента (ни `<ruby>`, ни `<rt>`), тогда как те же
теги через `document.createElement('ruby')`/`createElement('rt')` +
`appendChild` создаются и находятся `querySelector` без проблем — узкий
случай общего дефекта, не специфика ruby. 3 файла/сабтеста
(`position-relative.html`) добавлены к масштабу. `.ini`
`tests/wpt/metadata/css/css-ruby/position-relative.html`, `expected: FAIL`.
