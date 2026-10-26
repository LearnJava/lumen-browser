# BUG-1699 — multicol: повторная раскладка уже разрезанного контейнера дублирует фрагменты

**Статус:** OPEN
**Заведён:** 2026-10-24 (P1, при MULTICOL-VERTICAL)
**Область:** layout (`box_tree/multicol_trampoline.rs`, `box_tree/multicol_vertical.rs` — вход с уже разрезанными `children`)

## Симптом

Контейнер с `columns`, внутри которого блок режется между колонками (`multicol_flow::cut` делает из одного блока несколько фрагментов), при второй раскладке того же `LayoutBox` даёт вдвое больше фрагментов: второй проход берёт фрагменты первого за самостоятельные элементы и раскладывает каждый заново (`height`/`width` фрагмента — снова авторская). Повторную раскладку вызывает float, который не поместился в строке и переносится ниже.

## Проба

```html
<style>.cell{float:left;width:320px;height:330px;margin:0 8px 8px 0}
.m{columns:2;column-fill:auto;width:300px;height:100px}
.m>div{height:160px;border:6px solid #333;box-sizing:border-box}
.m>div>div{height:33px;margin:2px}</style>
<div style="width:700px"><div class="cell"></div><div class="cell"></div>
<div class="cell"><div class="m"><div><div></div><div></div><div></div><div></div></div></div></div></div>
```

`lumen --dump-layout`: у фона `#ffcc99` блока-обёртки 5 фрагментов вместо двух; без двух первых float'ов — 2. Тот же дефект в вертикальном режиме.

## Что делать

Раскладка multicol должна быть идемпотентной: либо хранить исходные (до разреза) дети контейнера и брать их при повторном входе, либо склеивать фрагменты обратно. Проверить, кто вызывает `lay_out` дважды для переносимого float.

## Как проверить

Проба выше (2 фрагмента при любом числе предшествующих float'ов); `graphic_tests/168-multicol-vertical-writing-mode.html` (ячейка D) держится на абсолютном позиционировании ячеек именно из-за этого дефекта.
