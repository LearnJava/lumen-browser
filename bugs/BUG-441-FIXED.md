# BUG-441 — присвоение `element.value` из скрипта не доезжает ни до рендера, ни до отправки формы

**Статус:** FIXED 2026-08-04
**Компонент:** dom (`Document::dirty_values` + `control_value`/`set_control_value`/`clear_control_value`), js (`dom.rs` — сеттер/геттер `value` в `_lumen_make_element`; `v8_runtime.rs` — нативы `_lumen_{get,set,clear}_dirty_value`), layout (`box_tree.rs`, `style.rs`), shell (`forms.rs`, `main.rs`), driver (`session.rs`)
**Найден:** 2026-07-29, P1, при починке [BUG-436](BUG-436-FIXED.md)
**Исправлен:** 2026-08-04, P3

## Симптом

```
eval  document.getElementById('inp').value = 'ZZ'   → "ZZ"
eval  document.getElementById('inp').value          → "ZZ"     ← читается верно
resource://screenshot                               → поле ПУСТОЕ
```

Значение жило только в JS-тени. Соответственно:

* поле не отрисовывалось с текстом;
* `forms::submit_form` собирал данные из `value`-атрибута DOM, то есть форма
  отправляла старое (пустое) значение;
* сценарий «скрипт предзаполняет форму» не работал в принципе.

## Репро

```html
<input id="inp" type="text">
```

```bash
target/dev-release/lumen.exe --mcp-live-port 9224 --no-scrollbar about:blank
```

```
navigate file:///.../page.html
wait     document_ready
eval     document.getElementById('inp').value = 'ZZ'
<читаем resource://screenshot>   → поле пустое
```

## Причина

Шим хранил значение контрола в карте `_input_values[nid]`:

```js
get value() {
    if (_input_values[nid] !== undefined) return _input_values[nid];
    var av = _lumen_u2n(_lumen_get_attr(nid, 'value'));
    return av !== null ? av : '';
},
set value(v) { _input_values[nid] = String(v); },
```

Сеттер не трогал DOM, а вёрстка и сбор формы читали именно `value`-атрибут.
Два источника истины расходились, и «истина для экрана» проигрывала.

Дописать `_lumen_set_attr(nid, 'value', v)` в сеттер было нельзя: IDL-атрибут
`value` **не** отражается в content-атрибут (HTML LS §4.10.5.5, dirty value
flag), иначе `form.reset()` и `<input value="…">` перестают различаться.

## Что сделано

Заведено единое runtime-хранилище значения контрола — `Document::dirty_values`
(`HashMap<NodeId, String>`, сериализуется вместе с документом, чтобы набранный
текст переживал гибернацию вкладки). Наличие записи **и есть** dirty value flag:

* `Document::control_value(id) -> Cow<str>` — текущее значение: dirty value,
  иначе дефолт (`value`-атрибут у `<input>`, текст детей у `<textarea>`);
* `Document::set_control_value` / `clear_control_value` — единственный путь
  записи и сброса. `value`-атрибут и текстовые дети остаются **дефолтом**,
  который читает `defaultValue` и восстанавливает `form.reset()`.

Читатели переведены на `control_value`:

| Слой | Что читает |
|---|---|
| layout | `box_tree.rs` — текст `<input>`, содержимое `<textarea>`, значение `type=range`; `style.rs` — `:placeholder-shown`, `:in-range`/`:out-of-range` |
| dom | `collect_dom_form_fields` (сбор данных формы), `element_validity` (valueMissing / typeMismatch / tooLong / tooShort) |
| shell | `typeable_field` (движковый ввод), date-picker, спеллчек |
| driver | `dispatch_type` (headless-ввод) |

Писатели: JS-сеттер `el.value` (через новый натив `_lumen_set_dirty_value`,
поднимающий `dom_dirty` и `record_dom_touch` — значение влияет на рестайл),
`forms::set_value`/`set_textarea_text` (движковый ввод, пикеры, спеллчек),
`InProcessSession::dispatch_type`, `_lumen_set_field_value` (синк из BUG-436).
`HTMLFormElement.reset()` и GC-тик мёртвых узлов чистят запись.

Побочно исправлено:

* `<textarea>` в `collect_dom_form_fields` читал несуществующий `value`-атрибут
  и всегда отдавал `""` — теперь отдаёт текст/значение;
* `forms::set_textarea_text` переписывал текстовых детей, то есть ввод
  уничтожал default value; теперь дети неприкосновенны;
* валидация и `:placeholder-shown`/`:in-range` видят набранное, а не
  разметочный дефолт;
* наложение `FormState` в `collect_form_entries` больше не перекрывает
  контрол с собственным runtime-значением (снимок `FormState` не знает о
  записях из JS и переживает `form.reset()`).

## Регресс-тесты

* `lumen-dom`: `control_value_falls_back_to_default_then_follows_dirty_value`,
  `control_value_of_textarea_shadows_child_text`,
  `collect_dom_form_fields_uses_runtime_value`, `validity_reads_runtime_value`;
* `lumen-layout`: `box_tree::tests::form_control_paints_runtime_value_over_the_default`;
* `lumen-js`: `input_value_assignment_reaches_document`,
  `form_reset_drops_document_side_value`;
* `lumen-shell`: `set_value_leaves_default_value_attribute_intact`,
  `set_textarea_text_shadows_default_value`.

## Осталось рядом

[BUG-444](BUG-444-FIXED.md) — та же болезнь у checkedness, починена
2026-08-30 тем же механизмом: хранилище в `Document` рядом с `dirty_values`.
Рядом также [BUG-383](BUG-383-OPEN.md) (рефлексия IDL-атрибутов форм в целом).
