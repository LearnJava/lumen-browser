# BUG-1057: `document.all` missing its spec-mandated `[[IsHTMLDDA]]` "unusual behaviors"

**Статус:** FIXED 2026-09-19 (P1, GAP-DOCALLDDA)
**Тип:** ДОРАБОТКА — требует расширения биндинга `rusty_v8` (нативный V8-примитив `MarkAsUndetectable`, которого нет в поверхности крейта), не JS-only правку
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` -- `document.all` absent entirely; `rusty_v8` binding -- no `MarkAsUndetectable` exposed)
**Найден:** P3 при работе над BUG-606, 2026-09-16

## Симптом

```
FAIL 'unusual behaviors' of document.all - assert_true: expected true got false
```
(`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/document-all.html`)

## Причина

HTML LS §obsolete requires `document.all` to be an `HTMLAllCollection` carrying
the `[[IsHTMLDDA]]` internal slot: `typeof document.all === "undefined"`, loose
equality to both `null` and `undefined`, falsy in boolean context, and calling
it as a function returns `undefined`. This is not reproducible from JS alone --
a `Proxy` has no `typeof` trap, so no shim-side object can make `typeof` report
`"undefined"`. Real engines implement it as a genuine internal slot; in V8
terms that is `v8::ObjectTemplate::MarkAsUndetectable()` (confirmed present in
the vendored V8 C++ headers shipped inside the `v8` crate,
`v8-150.1.0/v8/include/v8-template.h:1099`), but the `rusty_v8` Rust binding
(`v8-150.1.0/src/*.rs`) exposes no equivalent -- `grep -rn "undetectable"`
over that directory is empty.

Fixing this requires extending the `rusty_v8` binding itself (upstream PR or
a local patch) so `crates/js`'s native install path can mark an object
undetectable, then wiring `document.all` through it. Not a shim-only,
JS-level fix like the rest of BUG-606's scope, which is why it was split off.

## Масштаб

1 subtest, `document-all.html`. Self-contained -- no other WPT category in
this corpus depends on the DDA slot.

## Починка (2026-09-19, P1)

Ни upstream-PR ([denoland/rusty_v8#2078](https://github.com/denoland/rusty_v8/pull/2078),
открыт 2026-09-18), ни пин на форк починить это не могли, и причина тут же
объясняет, почему задача так долго стояла: в `build.rs` крейта `v8` вызов
`build_binding()` (компиляция `binding.cc`) стоит ТОЛЬКО под `V8_FROM_SOURCE`,
а дефолтный путь скачивает готовую `rusty_v8.lib`, собранную в CI denoland. Патч
чужого `binding.cc` в такой сборке не компилируется вообще — потребитель получил
бы Rust-код, зовущий несуществующий символ. (Плюс ветка форка сделана от
`152.2.0`, а воркспейс требует `150.1.0` — `[patch.crates-io]` её и не принял бы.)

Зато сам метод V8 в библиотеке уже есть: `grep` по
`target/dev-release/gn_out/obj/rusty_v8.lib` находит
`?MarkAsUndetectable@ObjectTemplate@v8@@QEAAXXZ`. Поэтому обёртка сделана своя —
[`crates/js/cpp/undetectable.cc`](../crates/js/cpp/undetectable.cc), одна единица
трансляции, которую собирает новый `crates/js/build.rs` (`cc`, только под фичей
`v8-backend`). Заголовки V8 намеренно НЕ подключаются: класс объявлен минимальной
заглушкой, задача которой — заставить компилятор породить то же манглированное
имя. Так сборка не зависит ни от раскладки исходников крейта `v8` (у него нет
ключа `links`, спросить include-путь у cargo нечем), ни от его build-time
дефайнов (`V8_COMPRESS_POINTERS` и пр.), а через границу ходит только указатель
на невиртуальный не-inline метод.

Два открытия по ходу:

1. **Undetectable обязан быть вызываемым.** V8 падает на
   `CHECK(!IsUndefined(obj->GetInstanceCallHandler()))` в
   `api-natives.cc:686` при инстанцировании шаблона — `[[IsHTMLDDA]]` в V8
   определён только для callable-объектов. Поэтому связаны ДВА символа, и
   `[[Call]]` (`document.all('id')` ≡ `namedItem`) реализован не как бонус, а
   как условие работоспособности.
2. **Обёртка falsy по построению — и `||` её съедает.** Первая версия шима
   писала `_lumen_make_html_all_collection(coll) || coll`, и фолбэк молча
   возвращал обычную коллекцию: успешный результат ложен в булевом контексте.
   Отличать «получилось» от «не получилось» можно только строгим сравнением
   (`!== undefined && !== null`) — единственным оператором, которому DDA-слот не
   врёт.

Сам `document.all` собран в
[`crates/js/src/v8_runtime/html_all.rs`](../crates/js/src/v8_runtime/html_all.rs):
нативная обёртка несёт только DDA-бит и обработчик вызова, а перехватчики
(named/indexed/query) переадресуют чтения живой коллекции, которую шим строит
существующим `_lumen_make_nid_collection` — поэтому `length`, индексы, `item()`,
`namedItem()` и именованный доступ живые и не продублированы.

Проверка: 6 тестов в
`crates/js/src/dom/tests/v8_bug1057_document_all_dda.rs` (оба сабтеста WPT
построчно, плюс идентичность, живость и `[[Call]]`) и сам прогон WPT —
`document-all.html` 2/2, ожидания `FAIL` удалены из
`tests/wpt/metadata/.../document-all.html.ini`, `--check` по каталогу даёт 0
регрессий.
