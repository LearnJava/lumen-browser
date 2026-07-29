# BUG-441 — присвоение `element.value` из скрипта не доезжает ни до рендера, ни до отправки формы

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — сеттер `value` в `_lumen_make_element`), shell/layout (нет runtime-хранилища значения, которое читают вёрстка и `forms::submit_form`)
**Найден:** 2026-07-29, P1, при починке [BUG-436](BUG-436-FIXED.md)

## Симптом

```
eval  document.getElementById('inp').value = 'ZZ'   → "ZZ"
eval  document.getElementById('inp').value          → "ZZ"     ← читается верно
resource://screenshot                               → поле ПУСТОЕ
```

Значение живёт только в JS-тени. Соответственно:

* поле не отрисовывается с текстом;
* `forms::submit_form` собирает данные из `value`-атрибута DOM, то есть форма
  отправит старое (пустое) значение;
* сценарий «скрипт предзаполняет форму» не работает в принципе.

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

Шим хранит значение контрола в карте `_input_values[nid]`:

```js
get value() {
    if (_input_values[nid] !== undefined) return _input_values[nid];
    var av = _lumen_u2n(_lumen_get_attr(nid, 'value'));
    return av !== null ? av : '';
},
set value(v) { _input_values[nid] = String(v); },
```

Сеттер не трогает DOM, а вёрстка и сбор формы читают именно `value`-атрибут.
Два источника истины расходятся, и «истина для экрана» проигрывает.

## Чего делать НЕ надо

Просто дописать `_lumen_set_attr(nid, 'value', v)` в сеттер — неверно по спеке:
IDL-атрибут `value` **не** отражается в content-атрибут (HTML LS §4.10.5.5,
dirty value flag), иначе `form.reset()` и `<input value="…">` перестают
различаться.

## Как чинить

Завести отдельное runtime-хранилище значения контрола (у шелла оно уже частично
есть — `Lumen::form_state`), сделать его единственным источником для вёрстки и
для `forms::collect_form_entries`/`submit_form`, а `value`-атрибут оставить
default value. Тогда и JS-сеттер, и движковый default action из
[BUG-436](BUG-436-FIXED.md) пишут в одно место.

Рядом: [BUG-383](BUG-383-OPEN.md) (рефлексия IDL-атрибутов форм в целом).
