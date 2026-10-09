# BUG-1590 — `ruby-overhang` не реализовано: свойство не разбирается, аннотация не заходит на соседний текст

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** css-parser/layout (`ruby-overhang`: в `crates/engine/layout/src/box_tree/build.rs:43` только комментарий со ссылкой на WPT)

## Симптом

Свойство `ruby-overhang: auto | none | spaces` (CSS Ruby 1 §«ruby-overhang») не заведено: нет разбора, нет вычисленного значения, нет раскладки. В `CSS-SPECS.md` (CSS Ruby L1) оно не значится. Следствие — аннотации, шире базы, всегда расширяют строку, а не нависают над соседними пробелами и знаками препинания.

## Проба

`--dump-layout` + `console.log`:

| вызов | у нас | ожидается |
|---|---|---|
| `CSS.supports("ruby-overhang","spaces")`, `("ruby-overhang","auto")`, `("ruby-overhang","none")` | `false` ×3 | `true` ×3 |
| `d.style.rubyOverhang = "spaces"; d.style.getPropertyValue("ruby-overhang")` | `spaces` (принято без проверки) | `spaces` |
| `d.style.rubyOverhang = "auto none"` | `auto none` | `""` |
| `getComputedStyle(ruby).getPropertyValue("ruby-overhang")` | `""` | `auto` |

## Как найдено

WPT-RUN-14 срез 27: `css/css-ruby/ruby-overhang-spaces-001…`, `parsing/ruby-overhang-{valid,invalid}.html`, `inheritance.html`.

## Что делать

Строка в `CSS-SPECS.md` (CSS Ruby L1) для P4: разбор и наследование (`ruby-overhang` наследуется), `ComputedStyle`, `getComputedStyle`; раскладка в `lay_out_ruby` — `auto` (нависание над соседним текстом без удвоения), `spaces` (только над пробельными символами, включая U+3000), `none`. Вертикальный и `ruby-align` варианты (11 id) — после горизонтального.

## Как проверить

`css/css-ruby/ruby-overhang-spaces-001.html`, `parsing/ruby-overhang-valid.html`, `ruby-overhang-none.html`.
