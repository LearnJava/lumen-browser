# BUG-1148 — немедленная загрузка картинки из скрипта не покрывает srcset/picture, iframe и гибернацию

**Статус:** FIXED 2026-10-01 (P3)
**Заведён:** 2026-09-24 (P6, при закрытии [BUG-1118](BUG-1118-FIXED.md)).
**Область:** js (`crates/js/src/v8_runtime/install/dom_core.rs` — `_lumen_set_attr` и
`queue_pending_img_loads` смотрят только на атрибут `src`), shell (`crates/shell/src/frames.rs`,
`crates/shell/src/tab_lifecycle/hibernate.rs` — `V8JsRuntime` строится без `with_image_load_hook`).

## Симптом

BUG-1118 ввёл `lumen_core::ext::ImageLoadHook`: присвоение `<img>.src` или вставка готового `<img src>`
в документ верхнего уровня ставит загрузку сразу, не дожидаясь relayout. Вне этого случая загрузка
по-прежнему ждёт relayout после окончания скрипта (`Lumen::spawn_dynamic_image_loads`):

- присвоение `srcset`/`sizes`, `<picture>` с `<source srcset>` — хук читает `src` буквально и
  не зовёт picker выбора источника;
- любая картинка, созданная скриптом внутри iframe-документа;
- вкладка, восстановленная после гибернации, — её рантайм собирается заново без хука.

## Что сделать

HTML LS §4.8.4.3 «update the image data» ставит загрузку при мутации `src`, `srcset`, `sizes`,
`crossorigin`, `referrerpolicy` и при изменении `<source>` родительского `<picture>`. Прогнать выбор
источника (тот же picker, что у relayout-прохода) прямо в хуке и передать хук в рантаймы фреймов и
восстановленной вкладки. Критерий — стенд BUG-1118 с `srcset` вместо `src` и тот же стенд внутри
`<iframe srcdoc>`: картинка запрашивается не позже следующего за ней `fetch()`.

## Исправление

`ImageLoadHook::viewport()` отдаёт рантайму вьюпорт для `sizes`; `_lumen_set_attr` на `src`/`srcset`/`sizes`
у `<img>` и `srcset`/`sizes`/`media`/`type`/`src` у `<source>` под `<picture>` зовёт
`lumen_layout::pick_image_request_url` (тот же picker, что у `collect_image_requests`) и ставит загрузку.
Хук передан рантаймам `<iframe>` (базa фрейма, отдельный `ImageRequestLedger`) и восстановленной вкладки
(общий с `Lumen` реестр). Тесты — `crates/js/src/dom/tests/v8_bug1148_srcset_img_load.rs`.

Не закрыто: `load`/`error` отсоединённой картинки во фрейме — [BUG-1230](BUG-1230-OPEN.md).
