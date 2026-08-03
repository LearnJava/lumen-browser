# Очередь S12b — добивание rquickjs из `crates/js` (P1)

**Developer:** P1
**Крейты:** `lumen-js` (в финальной группе — `lumen-shell`)
**Родитель:** `P3-v8-s12b` в [`ROADMAP.md`](../../ROADMAP.md), общий бриф — [`ph3-v8-migration.md`](ph3-v8-migration.md)
**Ветки:** `p1-s12b-<id>` (например `p1-s12b-b3`, `p1-s12b-g1`, `p1-s12b-f2`)

Один батч = одна строка в `STATUS-P1.md` = одна сессия **Sonnet**. Батчи нарезаны по
бюджету «строк исходника + тестов на порт», а не по смыслу — внутри батча модули
независимы, порядок внутри роли не имеет.

---

## 1. Контекст

V8 — движок по умолчанию с S12 ([ADR-018](../decisions/ADR-018-v8-cutover.md)),
rquickjs остаётся опциональным откатом и сносится срез за срезом. Срезы S12b-1…S12b-24
закрыты (шаблон — S12b-1 `badging.rs`, финальный — S12b-24 портирование 1128 тестов
`dom.rs` на `V8JsRuntime`). Этот файл — перепись **всего оставшегося** и его нарезка.

### Перепись на 2026-08-03 (метод воспроизводим)

```bash
cd crates/js/src
for f in *.rs wasm/*.rs; do
  c=$(grep -vE "^\s*(//|\*)" "$f" | grep -c "rquickjs")     # без комментариев!
  [ "$c" -gt 0 ] && echo "$(wc -l < "$f")|$(grep -c '#\[test\]' "$f")|$f"
done | sort -t'|' -k1 -n
```

**92 файла.** Наивный `grep -rl rquickjs` даёт 112 — разница целиком в doc-комментариях
вида «V8 port of the former rquickjs `install_*`», оставленных прошлыми срезами
(`badging.rs`, `battery_bindings.rs` и ещё ~20). **Фильтр комментариев обязателен**,
иначе батч наполовину состоит из уже закрытых модулей.

Из 92: 77 модулей уже имеют V8-порт (группа **A**, чистое удаление), 13 V8-порта не
имеют вовсе (группа **G**), 2 — `dom.rs` и `lib.rs` — финальная группа **F**.

### Находка, которую нельзя потерять

**13 модулей установлены только под QuickJS — под дефолтным (V8) движком этих Web API
нет вообще.** Проверено: `grep -rn "<модуль>::" crates/js/src/v8_runtime.rs crates/js/src/dom.rs`
→ 0 совпадений, при этом `lib.rs` (`QuickJsRuntime::install_dom`) их вызывает:

| Модуль | Строк | Тестов | Точка установки под QuickJS | Решение (G0, 2026-08-03) |
|---|---:|---:|---|---|
| `contacts.rs` | 110 | 4 | `lib.rs:937` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `background_sync.rs` | 160 | 5 | `lib.rs:995` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `periodic_sync.rs` | 164 | 4 | `lib.rs:1002` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `storage_buckets.rs` | 238 | 8 | `lib.rs:1024` | **порт** — [BUG-547](../../bugs/BUG-547-OPEN.md), CAPABILITIES.md overclaim |
| `push_api.rs` | 255 | 7 | `lib.rs:1009` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `background_fetch.rs` | 257 | 6 | `lib.rs:988` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `payment_request.rs` | 286 | 6 | `lib.rs:944` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `media_stream_recording.rs` | 297 | 8 | `lib.rs:1199` | **порт** — [BUG-549](../../bugs/BUG-549-OPEN.md) |
| `view_transitions.rs` | 329 | 11 | `lib.rs:1123` | **порт** — [BUG-545](../../bugs/BUG-545-OPEN.md), ROADMAP «done»/CAPABILITIES overclaim; JS-триггер отделён от движкового механизма (см. ниже) |
| `cookie_store.rs` | 383 | 8 | `lib.rs:1016` | **порт** — [BUG-546](../../bugs/BUG-546-OPEN.md), CAPABILITIES.md overclaim |
| `cookie_banner.rs` | 451 | 16 | `lib.rs:1030` | **порт** — [BUG-548](../../bugs/BUG-548-OPEN.md), пользовательский тумблер `ToggleCookieBannerDismiss` сейчас no-op |
| `webgl_bindings.rs` | 564 | 21 | — | **снос, не порт** — мёртвый код на ОБОИХ движках (`install_webgl_bindings` не вызывается вообще нигде вне своих тестов), вытеснен `webgl_canvas.rs` (функциональный WebGL, V8-портирован, сохраняет ADR-007 fingerprint-нормализацию). Бага не заведено — не регресс, просто устаревший модуль; удаление — обычный batch группы A следующего среза |
| `audio_bindings.rs` | 1120 | 29 | `lib.rs:765` (`new_session_seed`) | **снос, не порт** — [BUG-550](../../bugs/BUG-550-OPEN.md): уже затенён `web_audio.rs` (устанавливается позже в том же контексте, `globalThis.AudioContext` переписывается) ещё ДО V8-перехода; не регресс миграции. Функциональный пробел (доп. типы узлов + ADR-007 audio-noise) — отдельное решение, не в объёме S12b |

Т.е. `navigator.contacts`, `cookieStore`, `PaymentRequest`, `MediaRecorder`,
`document.startViewTransition` и др. в дефолтной сборке отсутствуют. Для 11 из 13
модулей это настоящий функциональный регресс V8-перехода (баги
BUG-545…BUG-549 заведены выше); `webgl_bindings.rs` и `audio_bindings.rs` — исключение,
уже мёртвый/затенённый код на обоих движках, в порте не нуждается (детали в таблице).
`view_transitions` при этом отдельно живёт на стороне движка (CSS View Transitions,
`P2-viewtrans` = done в ROADMAP, хотя фактически JS-триггер сейчас недоступен под V8) —
G0 разделил «JS API» (`view_transitions.rs`, нуждается в порте) и «движковый механизм»
(рендер-пайплайн, V8-агностичен, не тронут).

---

## 2. Процедура батча группы A (удаление, V8-порт уже есть)

Шаблон — findings-запись S12b-1 в [`ph3-v8-migration.md`](ph3-v8-migration.md). Для
**каждого** модуля батча:

1. **Проверить, что порт правда есть:**
   `grep -nE "fn [a-z_]*_v8" crates/js/src/<модуль>.rs` — либо
   `grep -n "<модуль>::" crates/js/src/v8_runtime.rs` (у части модулей V8-версия
   заинлайнена в `v8_runtime.rs`: `trusted_types`, `subtle_crypto`, `canvas2d`).
   Порта нет → модуль не из группы A, вернуть в очередь и написать об этом в отчёте.
2. **Найти все тесты модуля, а не только его `mod tests`** (ловушка S12b-6/S12b-9):
   `grep -n "<file_stem>_" crates/js/src/dom.rs | head` — у `pip_bindings` 12 тестов
   жили в `dom.rs`, а не в своём файле.
3. **Портировать покрытие на V8** — новые тесты против `V8JsRuntime` +
   `install_<модуль>_v8`, под `#[cfg(all(test, feature = "v8-backend"))]`.
   Тест-за-тест, счётчик не уменьшать; если тест проверял чисто rquickjs-специфику
   (тип ошибки движка, `rquickjs::Value`), удалить с обоснованием в теле коммита.
   Готовые формы харнесса: голый `V8JsRuntime::new()` (S12b-12/13) или
   `V8JsRuntime::new()` + полный `install_dom`, если шим опирается на `DOMException`
   и прочие классы (S12b-11).
4. **Удалить rquickjs-сторону:** `install_*`/`init_*` функцию, `use rquickjs::…`,
   старый `mod tests`.
5. **Снять вызов** в `QuickJsRuntime::install_dom` (`crates/js/src/lib.rs`).
6. **SHIM-константу** пометить `#[cfg(feature = "v8-backend")]`, если после удаления её
   читает только V8-путь (S12b-12/13/14).
7. `pub mod <модуль>;` в `lib.rs` **оставить** — в файле живёт V8-функция.

### Гейт батча

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
cargo clippy -p lumen-js --all-targets -- -D warnings
cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings
cargo test  -p lumen-js --features v8-backend <модуль1> <модуль2> …
cargo test  -p lumen-js                                        # rquickjs-суита не покраснела
```

Частая мелочь: после превращения `//`-шапки в `//!` срабатывает clippy
`empty_line_after_doc_comments` (S12b-5/8/10/12) — убрать пустую строку.

### Definition of done (батч A)

- [ ] Ни одного не-комментарного `rquickjs` в файлах батча (перепись из §1 их не находит)
- [ ] Оба clippy чистые, обе тест-суиты зелёные
- [ ] В `ph3-v8-migration.md` дописана findings-запись батча (что удалено, сколько тестов портировано/удалено и почему)
- [ ] Строка батча удалена из `STATUS-P1.md`, строка `ROADMAP.md` → `done`

---

## 3. Группа A — батчи удаления

Формат: `модуль (строк/тестов)`. Суммы — бюджет сессии.

### Полоса 1 — мелкие модули, 5 штук на батч

| id | Модули | Σ строк | Σ тестов |
|---|---|---:|---:|
| **S12b-B1** | `trusted_types` (143/0), `typed_om_api` (148/0), `serial` (151/4), `scroll_snap_events` (179/5), `webxr` (210/4) | 831 | 13 |
| **S12b-B2** | `soft_navigation` (217/5), `bluetooth` (219/7), `eye_dropper` (221/6), `virtual_keyboard` (225/5), `local_font_access` (226/8) | 1108 | 31 |
| **S12b-B3** | `sanitizer` (230/8), `ua_client_hints` (243/4), `reporting_api` (246/6), `launch_handler` (256/9), `storage_manager` (260/10) | 1235 | 37 |
| **S12b-B4** | `webhid` (260/7), `network_log_bindings` (262/8), `css_properties_values_api` (265/7), `scheduler` (288/5), `paint_worklet` (294/8) | 1369 | 35 |
| **S12b-B5** | `presentation_api` (298/6), `screen_orientation` (298/8), `window_management` (313/8), `navigation_api` (315/0), `speech` (315/0) | 1539 | 22 |
| **S12b-B6** | `iframe_element` (315/10), `url_pattern` (315/5), `web_midi` (317/11), `surface_api` (325/11), `scroll_timeline` (327/14) | 1599 | 51 |

`typed_om_api`, `serial`, `scroll_snap_events` в прошлых срезах помечены как «ловушки»
(тесты живут в `dom.rs`) — в B1 шаг 2 процедуры обязателен, не факультативен.

### Полоса 2 — средние, 3 штуки на батч

| id | Модули | Σ строк | Σ тестов |
|---|---|---:|---:|
| **S12b-B7** | `web_locks` (331/6), `webusb` (347/8), `close_watcher` (361/8) | 1039 | 22 |
| **S12b-B8** | `gamepad` (366/16), `generic_sensor` (384/16), `shared_storage` (389/13) | 1139 | 45 |
| **S12b-B9** | `element_internals` (411/7), `video_pip` (414/11), `media_session` (419/15) | 1244 | 33 |
| **S12b-B10** | `long_animation_frames` (428/10), `form_validation` (432/7), `wake_lock` (461/12) | 1321 | 29 |
| **S12b-B11** | `navigator_bindings` (491/16), `media_capture` (507/8), `screen_capture` (521/11) | 1519 | 35 |
| **S12b-B12** | `geolocation` (535/17), ~~`esm`~~ (543/17, pulled — see below), `idle_detection` (560/17) | 1638 | 51 |
| **S12b-B13** | `broadcast_channel` (562/14), `webrtc_stub` (588/17), `credentials` (623/12) | 1773 | 43 |
| **S12b-B14** | `xhr` (655/17), `file_input` (657/18) | 1312 | 35 |

`broadcast_channel`, `geolocation`, `idle_detection`, `file_input`, `media_capture`,
`screen_capture` — модули с натив-состоянием в `V8JsRuntime` (поля/аксессоры добавлены
в S5–S7 батче 3). Их V8-тесты требуют не голого контекста, а полного `install_dom`.
(На практике `geolocation`/`idle_detection` в S12b-B12 хватило голого `V8JsRuntime::new()`
+ локальных стабов — общий `install_dom` не понадобился.)

**`esm` исключён из S12b-B12** (закрыт 2026-08-04) — не подходит под процедуру группы A:
его rquickjs-сторона (`impl Resolver`/`impl Loader` для `LumenResolver`/`LumenLoader`)
не ставится через `install_dom`, а вшита в конструктор `QuickJsRuntime::new()`/
`js_thread_main` (`lib.rs`) как основа ES-модульной подсистемы движка — снос требует
той же по масштабу правки, что и снос самого `QuickJsRuntime` (`S12b-F2`). Подробности
и обоснование — findings-запись S12b-B12 в `ph3-v8-migration.md`. `esm` больше не строка
очереди группы A — его rquickjs-часть уходит вместе с `QuickJsRuntime` в `S12b-F2`.

### Полоса 3 — крупные, 1–2 штуки на батч

| id | Модули | Σ строк | Σ тестов |
|---|---|---:|---:|
| **S12b-B15** | `web_codecs` (669/8), `decorators` (714/10) | 1383 | 18 |
| **S12b-B16** | `intl_bindings` (740/19), `media_devices` (845/24) | 1585 | 43 |
| **S12b-B17** | `wasm/mod` (849/0), `sw_worker` (877/6) | 1726 | 6 |
| **S12b-B18** | `es2026_proposals` (908/15), `shared_worker` (957/9) | 1865 | 24 |
| **S12b-B19** | `notifications_bindings` (913/26), `web_audio` (965/13) | 1878 | 39 |
| **S12b-B20** | `filesystem_access` (1042/33) | 1042 | 33 |
| **S12b-B21** | `webgl_canvas` (1045/13), `audio_element` (1072/18) | 2117 | 31 |
| **S12b-B22** | `video_bindings` (1145/12), `webassembly` (1163/15) | 2308 | 27 |
| **S12b-B23** | `dom_parser` (1208/19) | 1208 | 19 |
| **S12b-B24** | `temporal_api` (1226/30) | 1226 | 30 |
| **S12b-B25** | `svg` (1231/20) | 1231 | 20 |
| **S12b-B26** | `offscreen_canvas` (1313/23) | 1313 | 23 |

`offscreen_canvas` в записи S8 помечен как «не портирован» — это устарело:
`install_offscreen_canvas_bindings_v8` есть (`offscreen_canvas.rs:488`). Шаг 1 всё равно
выполнить.

### Полоса 4 — тяжёлые, по одному на батч

| id | Модуль | Строк | Тестов | Особенность |
|---|---|---:|---:|---|
| **S12b-B27** | `worker` | 1497 | 26 | 11 ссылок из `v8_runtime.rs`, состояние воркеров живёт в рантайме |
| **S12b-B28** | `tc39_proposals` | 1606 | 51 | самая большая тест-суита полосы; V8 нативно умеет часть предложений — часть тестов может стать проверкой движка, а не шима |
| **S12b-B29** | `webgpu` | 2503 | 29 | завязан на фичу `webgpu` (`lumen-paint/backend-wgpu`); гейт гонять и с ней |
| **S12b-B30** | `canvas2d` | 2691 | 31 | V8-версия заинлайнена в `v8_runtime.rs` (паттерн «б», `thread_local`); `flush_canvas_updates` трогать нельзя |
| **S12b-B31** | `subtle_crypto` | 2717 | 39 | V8-биндинги в `v8_runtime.rs:3804+`, зеркалят rquickjs-версию построчно |

---

## 4. Группа G — 13 модулей без V8-порта

### S12b-G0 — триаж (делать первым в группе, отдельная сессия)

Для каждого из 13 модулей §1 решить **порт или снос**, ничего не удаляя:

- жив ли API в `CAPABILITIES.md` и заявлен ли он как поддержанный;
- есть ли WPT-категория, которая на нём проваливается (`docs/wpt-status.md`, `.ini` в `tests/wpt/`);
- `webgl_bindings` — вытеснен ли `webgl_canvas.rs`; `audio_bindings` — вытеснен ли `web_audio.rs`;
  `view_transitions` — что из файла JS API, а что движковый механизм `P2-viewtrans`.

**Выход G0:** BUG-NNN на каждый модуль, признанный регрессом (заголовок вида «X отсутствует
в дефолтной (V8) сборке»), обновлённая таблица §1 с колонкой «решение», правка
`CAPABILITIES.md` там, где заявлено больше, чем есть. Модули со вердиктом «снос»
переезжают в группу A следующим батчем.

**Закрыт (P1, 2026-08-03):** 11 из 13 модулей — вердикт «порт», реальный
функциональный регресс V8-перехода: BUG-545 (`view_transitions` —
`document.startViewTransition` отсутствует, хотя `ROADMAP.md` помечает
`P2-viewtrans` done и `CAPABILITIES.md` числит его ✅; движковый механизм
кросс-фейда сам по себе не задет, отделён от JS-триггера), BUG-546/547
(`cookie_store`/`storage_buckets` — оба заявлены ✅ в `CAPABILITIES.md`,
правка внесена: понижены до 🟡 с пометкой «QuickJS-only»), BUG-548
(`cookie_banner` — пользовательский тумблер `KeyCommand::ToggleCookieBannerDismiss`
сейчас no-op под V8, уже был самодокументирован комментарием в
`shell/src/main.rs:19230` как известный, но не заведённый в трекер пробел),
BUG-549 (7 Phase-0 заглушек одним багом — `contacts`/`background_sync`/
`periodic_sync`/`push_api`/`background_fetch`/`payment_request`/
`media_stream_recording`: в `CAPABILITIES.md` не заявлены, но под QuickJS
были feature-detectable — `'X' in window` `true`-then-reject, под V8 —
`false`). 2 модуля — вердикт «снос», НЕ регресс миграции: `webgl_bindings.rs`
мёртв на обоих движках (`install_webgl_bindings` не вызывается вообще нигде
вне своих тестов, полностью вытеснен функциональным `webgl_canvas.rs`, баг
не заведён — нечего чинить); `audio_bindings.rs` (BUG-550) был уже затенён
`web_audio.rs` внутри QuickJS ДО V8-перехода — оба шима пишут в один
`globalThis.AudioContext` через присваивание (не `class`-декларацию), и
`web_audio` устанавливается вторым в том же `install_dom`, так что более
богатая версия `audio_bindings` (доп. типы узлов + ADR-007 antifingerprint
audio-noise) никогда не была доступна странице ни на одном движке —
находка, не связанная с S12b как таковым. Оба переезжают в группу A
(удаление, без порта) — таблица G1…G7 ниже обновлена.

### S12b-G1…G7 — порты

Состав после G0 (`webgl_bindings`/`audio_bindings` исключены — см. решение
«снос» в §1, оба ушли в группу A):

| id | Модули |
|---|---|
| **S12b-G1** | `contacts` (110/4), `background_sync` (160/5) |
| **S12b-G2** | `periodic_sync` (164/4), `storage_buckets` (238/8) |
| **S12b-G3** | `push_api` (255/7), `background_fetch` (257/6) |
| **S12b-G4** | `payment_request` (286/6), `media_stream_recording` (297/8) |
| **S12b-G5** | `view_transitions` (329/11), `cookie_store` (383/8) |
| **S12b-G6** | `cookie_banner` (451/16) |
| **S12b-Asnos1** | `webgl_bindings` (564/21) — снос без порта, обычная процедура группы A §2 (модуль уже не установлен ни у одного движка, шаг 1 её процедуры пропускается — порта не было и не нужен) |
| **S12b-Asnos2** | `audio_bindings` (1120/29) — снос без порта, та же процедура; см. BUG-550 про непортированный функциональный пробел |

Процедура порта: `install_<модуль>_v8` рядом с оригиналом, регистрация через `install_v8!`
в `v8_runtime.rs::install_dom` (сейчас там 89 вызовов), тесты против `V8JsRuntime`.
Модули без нативов (чистый `ctx.eval(SHIM)`) — это большинство списка — переносятся
заменой `rquickjs::Ctx::eval` на `lumen_core::ext::JsRuntime::eval`. Дальше модуль
проходит обычный батч группы A (удаление rquickjs-стороны) — можно в той же сессии,
если бюджет позволяет.

---

## 5. Группа F — финал (бывший `S12b-25`)

Строго после того, как перепись §1 не находит ничего, кроме `dom.rs` и `lib.rs`.

| id | Что | Ориентир |
|---|---|---|
| **S12b-F1** | `lumen-shell`: убрать фичу `quickjs` и ветки конструирования QuickJS-рантайма (`crates/shell/Cargo.toml:28`), упростить `any(quickjs, v8)`-гейты до безусловных | shell собирается без `quickjs`, `--features quickjs` больше не существует |
| **S12b-F2** | `lumen-js/lib.rs`: удалить `QuickJsRuntime`, `QuickPersistentJs`, `QuickJsRuntime::install_dom`, `rq_err` (`lib.rs:2211`), `__lum_args__`-костыль, `use rquickjs::…` (`lib.rs:144`); заодно — `esm.rs`: `impl Resolver`/`impl Loader` для `LumenResolver`/`LumenLoader` (rquickjs-специфика, вшита в `QuickJsRuntime::new()`/`js_thread_main`, исключена из S12b-B12 — см. §3 Полоса 2) | 2574 строки файла, 29 тестов; V8-путь не тронут; `esm.rs`'s `ImportMap`/`resolve_specifier_with` остаются — общие с `v8_esm.rs` |
| **S12b-F3** | `dom.rs`: удалить `install_primitives` (`dom.rs:460–3196`, 2736 строк) и `use rquickjs::{Ctx, Function, Result as QjResult}` (`dom.rs:16`) | тесты `dom.rs` уже на V8 (S12b-24) — порта не требуется, только проверка, что 1128 тестов зелёные |
| **S12b-F4** | `crates/js/Cargo.toml`: убрать `rquickjs` (строка 42), поправить `description` крейта («QuickJS implementation of the JsRuntime trait»), обновить `docs/plan/tech-stack.md`, `CAPABILITIES.md`, `subsystems/js.md`, `ADR-018`; `rquickjs` исчезает из `Cargo.lock` | `grep -rn rquickjs` по репозиторию — только исторические ADR/findings |

Порядок F1 → F2 → F3 обязателен: `install_primitives` жив, пока его зовёт
`QuickJsRuntime::install_dom`, а тот — пока shell умеет его конструировать.

После F4 — `P3-v8-post-audit` (уже отдельная строка в `ROADMAP.md`): пройти OPEN-строки
`BUGS.md` на связь с QuickJS.

---

## 6. Общие ловушки

- **`grep -rl rquickjs` врёт** на ~20 файлов (doc-комментарии прошлых срезов). Всегда
  фильтровать комментарии — иначе батч наполовину пустой.
- **Тесты модуля могут жить в `dom.rs`**, а не в своём файле (S12b-6/S12b-9). Проверять
  по file-stem перед удалением.
- **`cargo test -p lumen-js` без фич — это rquickjs-суита**, а не «все тесты». V8-суита —
  только с `--features v8-backend`. До финальной группы гонять обе.
- **Удаление вызова из `QuickJsRuntime::install_dom` меняет поведение QuickJS-сборки** —
  модуль в ней перестаёт устанавливаться. Это принято как намеренный побочный эффект
  шаблона (S12b-1), но упоминать в теле коммита.
- Батч трогает только `crates/js` — дисплей-лист не двигается, гейт = `scripts/scoped-test.sh`,
  полный графический прогон не нужен.

## 7. Бюджет сессии

Один батч = один коммит + одна findings-запись. Если по ходу выяснилось, что модуль
из группы A на самом деле без V8-порта (шаг 1) — **не портировать в этой же сессии**:
вынести модуль из батча, дописать его в группу G, закрыть батч оставшимися модулями.
Смешивание удаления и порта в одном коммите — ровно тот «half-finished deletion sweep»,
от которого предостерегает scoping-запись S12b.
