# BUG-434 — сабсеты `unicode-range` одной @font-face-семьи затирают друг друга в реестре шрифтов

**Статус:** FIXED 2026-08-05 (P3)
**Компонент:** font (`crates/engine/font/src/font_registry.rs` —
`FontRegistry::register_from_bytes`), paint (`crates/engine/paint/src/renderer.rs`
— `Renderer::resolve_face_id_uncached`/`load_face_by_record`,
`crates/engine/paint/src/backends/femtovg_backend.rs` — `resolve_font_chain`),
shell (`crates/shell/src/main.rs` — обработчик `LoadEvent::FontLoaded` +
`load_font_faces`)
**Найден:** P1, 2026-07-29 (выделен из [BUG-423](BUG-423-FIXED.md), где эта
гипотеза объясняла симптом «пропадают буквы» — и была опровергнута)
**Приоритет:** средний — текст рисуется, но не той гарнитурой и не с теми
метриками, которыми его измерили

## Что происходит

`register_from_bytes` ключует запись виртуальным путём
`@font-face:<family_lower>/<weight>/<style>` — `unicode-range` в ключ не входит.
Документированное поведение: «если для той же (family, weight, style) запись
уже есть — она заменяется: последнее правило wins». Для двух конкурирующих
@font-face это верно, но **сабсеты одной семьи не конкурируют — они дополняют
друг друга** (CSS Fonts L4 §5.1), и последний пришедший вытирает предыдущие.

`unicode_range` из `LoadEvent::FontLoaded` в реестр вообще не передаётся: он
уходит только в `web_fonts` → `MultiFontMeasurer`, у которого слоты на семью
уже сделаны правильно (`register_family_with_ranges`).

## Замер (2026-07-29)

Локальная страница с `fonts.googleapis.com/css2?family=Roboto` (Google отдаёт
семью 9 сабсетами c одной тройкой family/weight/style):

```
FontLoaded: «Roboto» weight=400        × 9
[fontreg] REPLACE @font-face:roboto/400/normal × 8   → в реестре остался 1 face
```

## Следствие

Измеритель считает ширины по всем сабсетам, а рендер знает один. Кодпоинты вне
его покрытия рисуются **чужим** face-ом — `renderer.rs::pick_face_for_codepoint`
идёт по всем загруженным face-ам подряд и берёт первый, где есть глиф (bundled
Inter, шрифт хрома, системный). Гарнитура и метрики расходятся с теми, по
которым посчитан layout.

**Буквы при этом не пропадают** — живой кадр такой страницы отрисован
полностью. Пропажа букв на реальных сайтах была отдельным дефектом
([BUG-423](BUG-423-FIXED.md), bbox composite-глифа).

## Что нужно сделать

1. Включить `unicode-range` в идентичность записи реестра (диапазон в
   виртуальном пути либо список сабсетов на семью).
2. Провести `unicode_range` из `FontLoaded` в реестр — сейчас он теряется на
   границе shell → font.
3. Рендер должен загружать **все** сабсеты выбранной семьи, иначе каскад по
   кодпоинту всё равно не найдёт нужный face: `resolve_face_id_uncached` берёт
   ровно один `pick_face`-хит.
4. В `pick_face_for_codepoint` — сначала перебирать face-ы своей семьи, потом
   остальные, иначе глиф уедет в первый попавшийся загруженный шрифт.
5. Регресс-тест в `lumen-font`: два @font-face одной семьи и веса с
   непересекающимися `unicode-range` — оба набора символов должны разрешаться в
   свои face-ы, а не в последний зарегистрированный.

## Фикс (2026-08-05, P3)

1. **Идентичность записи реестра** (`font_registry.rs::register_from_bytes`):
   виртуальный путь получил четвёртый сегмент — канонический ключ
   `unicode_range_key()` (`"all"` без дескриптора, иначе
   `"{start:x}-{end:x},…"`). Сигнатура приросла параметром
   `unicode_range: &[UnicodeRange]`; повторная регистрация того же диапазона
   по-прежнему заменяет запись (FOUT-рефетч), но другой диапазон той же
   (family, weight, style) больше не стирает предыдущий — оба сабсета
   сосуществуют в `custom: HashMap<String, Vec<FaceRecord>>`.
2. **Проводка диапазона** (`shell/main.rs`): `LoadEvent::FontLoaded`-обработчик
   уже нёс распарсенный `unicode_range: Vec<UnicodeRange>` — просто передан в
   `register_from_bytes`. `load_font_faces` (синхронный путь `local()`)
   парсит `rule.unicode_range` тем же `lumen_font::parse_unicode_ranges`
   перед вызовом.
3. **Загрузка всех сабсетов в рендер.** `FaceRecord` осознанно НЕ получил
   поле `unicode_range` — `core` не может зависеть от `font` (направление
   графа `core → font`), а декларативный диапазон и не нужен для выбора: cmap
   каждого face-а — надёжный источник истины о покрытии кодпоинта, к тому же
   Lumen и так качает все сабсеты эагерно (ленивой загрузки по диапазону нет).
   Вместо этого оба рендер-пути после резолва primary face теперь дозагружают
   через `provider.lookup_faces(fam)` все записи с тем же
   (weight, style, stretch), что и primary, но другим `path` — wgpu
   (`resolve_face_id_uncached`, вынесенный хелпер `load_face_by_record`)
   вставляет их в `self.faces`, femtovg (`resolve_font_chain`) добавляет их
   `FontId` сразу после primary в цепочку.
4. **Приоритет своей семьи в каскаде** — оказался не нужен отдельным пунктом:
   у wgpu-пути `pick_face_for_codepoint` уже сканирует `self.faces` по
   реальному покрытию cmap (не по декларативному diapазону), и после п.3 все
   сабсеты нужной семьи оказываются в этом списке; у femtovg-пути порядок
   `FontId` в цепочке и так соответствует порядку добавления семей, а сабсеты
   одной семьи теперь идут подряд перед следующей fallback-семьёй.
5. **Регресс-тесты** в `lumen-font::font_registry::tests`:
   `subsets_with_different_unicode_range_coexist` (два сабсета — оба
   выживают, оба доступны через `read_face_bytes`) и
   `re_registering_same_unicode_range_still_replaces` (повтор того же
   диапазона по-прежнему заменяет, не копится).

Полный `graphic_tests/run.py --continue-on-fail` (152 теста, чанками):
96 PASS, 51 DEBTOR (в пределах ратчета ±2 п.п., часть багов BUG-128
font-parity даже улучшилась — ожидаемо, рендер теперь честно берёт
покрывающий codepoint face вместо произвольного), 4 FAIL — все
предсуществующие относительно `results/latest.json` (2026-07-31): TEST-61
(BUG-103), TEST-147/150/151 (все три улучшились против устаревшего baseline,
не регрессия), 1 ERROR — TEST-112, транзиентный `WinError 10054`,
на повторном прогоне PASS.

## Связанные

* [BUG-423](BUG-423-FIXED.md) — исходная заявка, из которой это выделено.
* [BUG-170](BUG-170-FIXED.md) — предыдущая правка того же пути (FOUT).
