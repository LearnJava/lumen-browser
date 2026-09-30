# BUG-1229 — текст встроенного `<script>` берётся из потомков-элементов

**Статус:** OPEN
**Заведён:** 2026-09-30 (найден при BUG-1064)

## Симптом

```js
var s = document.createElement('script'); document.body.appendChild(s);
var sp = document.createElement('span'); sp.textContent = 'light';
s.append(sp);   // исполняется `light` → ReferenceError: light is not defined
```

Спека (HTML LS §4.12.1 «child text content») берёт только дочерние Text-узлы; span в счёт не идёт,
текст скрипта пуст.

## Масштаб

`shadow-dom/declarative/gethtml.html` получает harness ERROR «light is not defined» (элемент
`script` не допускает shadow, тест делает `script.append(<span>light</span>)`), отсюда же
периодический TIMEOUT файла. Baseline сейчас фиксирует ERROR.
