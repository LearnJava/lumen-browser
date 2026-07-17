# BUG-299 — `Element.prototype.insertAdjacentText` missing entirely

**Статус:** FIXED 2026-07-17
**Компонент:** js (`crates/js/src/dom.rs` shim)
**Найден:** P2-wpt S4, same diagnosis session as [BUG-298](BUG-298-FIXED.md)

## Симптом

`typeof document.createElement('div').insertAdjacentText === 'undefined'`; calling it throws `TypeError: insertAdjacentText is not a function`.

## Причина

Never implemented — `insertAdjacentHTML` isn't implemented either (unrelated to this bug; not needed by the code path that surfaced this). `testharness.js`'s `Output.prototype.show_results` → `get_asserts_output(test)` calls `asserts_output.querySelector("summary").insertAdjacentText("afterend", "No asserts ran")` whenever a test has no recorded assertions — a call the harness makes unconditionally on every synchronous `test()`-based WPT test, not an edge case. Same silent-swallow effect as [BUG-298](BUG-298-FIXED.md): the throw aborts the `completion` callback dispatch loop before `testharnessreport.js`'s own callback runs, with zero visible trace (`crates/js/src/dom.rs`'s blanket `try {...} catch(e) {}` pattern around callback dispatch).

## Фикс (2026-07-17)

Implemented directly in the shared JS shim (`WEB_API_SHIM`, `crates/js/src/dom.rs`), reusing the existing `before`/`prepend`/`append`/`after` primitives already on `Element.prototype` rather than adding a new native binding:

```js
insertAdjacentText: function(where, text) {
    var t = String(text);
    switch (String(where).toLowerCase()) {
        case 'beforebegin': this.before(t); break;
        case 'afterbegin':  this.prepend(t); break;
        case 'beforeend':   this.append(t); break;
        case 'afterend':    this.after(t); break;
        default: throw new SyntaxError('insertAdjacentText: invalid position ' + where);
    }
},
```

Matches DOM Parsing & Serialization §4's four insertion positions. Verified directly against a live `lumen --bidi-port` window: `d.insertAdjacentText('afterend', 'x')` on a detached `<div>` no longer throws.
