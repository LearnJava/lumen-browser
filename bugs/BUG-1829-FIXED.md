# BUG-1829-FIXED: фингерпринт-дивергенты toString-пробы BUG-1826 — SAB под crossOriginIsolated, прототипы WebGL/Crypto, RTCPeerConnection

- **Статус:** FIXED 2026-11-20
- **Область:** js (`crates/js/src` — `shim/sab_gate_shim.js`, `shim/crypto_shim.js`,
  `shim/web_api_shim_tail_mc.js`, `webgl_canvas.rs`, `dom.rs`)
- **Приоритет:** P2 (анти-бот скоринг: детерминированные дивергенции от Chrome)

## Симптом (исходная запись)

Проба 80 платформенных API (Lumen против CfT 157) нашла четыре структурные
дивергенции, видимые без `toString`:

1. `SharedArrayBuffer` виден на обычной странице; Chrome отдаёт `undefined`
   без `crossOriginIsolated`.
2. `WebGLRenderingContext.prototype.getParameter` / WebGL2-эквивалент
   отсутствуют — методы висели на инстансе, прототип пустой.
3. `Crypto.prototype` отсутствует — `crypto` был plain object.
4. `RTCPeerConnection` — `undefined` целиком.

## Разбор и фикс

### 1. SharedArrayBuffer — `crates/js/src/shim/sab_gate_shim.js` (новый)

V8 ставит интринсик безусловно. Шим-часть читает `_LUMEN_CROSS_ORIGIN_ISOLATED`
(тот же флаг, из которого `install_dom` строит `window.crossOriginIsolated` из
COOP/COEP через `lumen_network::CrossOriginIsolationState`) и на неизолированном
реалме делает `delete globalThis.SharedArrayBuffer` + страховочное затенение
`undefined`, чтобы `typeof` отвечал `'undefined'` даже при не-configurable
интринсике. `Atomics` остаётся — Chrome тоже держит неймспейс всегда, гейтится
только конструктор. Часть включена и в `worker_exposed_shim()`: воркерный
скоуп флаг не несёт (изоляция от creator'а в Lumen не распространяется —
зафиксированное ограничение, в Chrome воркер наследует изоляцию) → воркеры
всегда берут non-isolated ветку, что совпадает с Chrome для
неизолированного creator'а.

### 2. WebGL — `crates/js/src/webgl_canvas.rs`

Методы и константы перенесены с инстанса на прототипы: `_GL_METHODS` вешается
на `_GL1.prototype` и `_GL2.prototype`, `_GL_CONSTS`/`_GL2_CONSTS` — через
`_installConsts` (non-writable, non-configurable, как WebIDL),
`canvas`/`drawingBufferWidth`/`drawingBufferHeight` — accessor'ы
`_installAttrs` над per-context состоянием в `WeakMap` (`_glStates`).
Инстанс — `Object.create(_GL?.prototype)`, собственных свойств нет;
`gl instanceof WebGLRenderingContext` работает, `toStringTag` сохранён.
Бренд-чек: `_glState(this)` бросает TypeError на чужом `this` — как
`Illegal invocation` в Chrome.

### 3. Crypto — `crates/js/src/shim/crypto_shim.js`

`crypto` — `Object.create(Crypto.prototype)`; `Crypto` — функция с
нелегальным конструктором, `getRandomValues`/`randomUUID` на прототипе.
`SubtleCrypto` — интерфейс с литеральным прототипом (`constructor` и
`Symbol.toStringTag` прописаны явно), `crypto.subtle` — отдельный экземпляр
через `Object.create(SubtleCrypto.prototype)`. Гейты `[SecureContext]` для
`subtle`/`CryptoKey` сохранены (`exposeSecure` на secure-реалме,
`_lumen_crypto_expose_secure` для воркеров).

### 4. RTCPeerConnection — `crates/js/src/shim/web_api_shim_tail_mc.js`

Честный интерфейс-заглушка (не фейковый медиа-стек): конструктор требует `new`,
наследует `EventTarget`, `[SecureContext]`-гейт как в Chrome. Прототип несёт
readonly-атрибуты (`signalingState`, `connectionState`, `iceConnectionState`,
`iceGatheringState`, description-поля, `sctp`, `canTrickleIceCandidates`) и
`on*`-хендлеры; `getSenders`/`getReceivers`/`getTransceivers` — честные `[]`;
`close()` переводит состояние в `closed`. Всё, что требует отсутствующего
WebRTC-стека, падает честно: async-методы (`createOffer`, `createAnswer`,
`setLocalDescription`, `setRemoteDescription`, `addIceCandidate`, `getStats`)
reject'ят `NotSupportedError`, синхронные (`addTrack`, `removeTrack`,
`addTransceiver`, `restartIce`) бросают `DOMException NotSupportedError`;
`generateCertificate` — rejected promise. Illegal-invocation проверки на
прототипных методах — как WebIDL.

### Sweep

Все новые интерфейсные методы достижимы из глобалов
(`WebGLRenderingContext`, `Crypto`, `RTCPeerConnection`), поэтому попадают
под `_lumen_native_sweep()` — она вызывается после всех `install_*`
(`v8_runtime.rs`, BUG-1826/WS-A), `toString` печатает `[native code]`, как в
Chrome.

## Проверки

- `cargo test -p lumen-js --features v8-backend --lib` по затронутым модулям
  (`v8_webcrypto`, `v8_idle_message_clipboard`, `v8_worker1_exposed`,
  `webgl_canvas::tests_v8`, splice-тесты `dom::tests::shim`): 118 зелёных.
  Новые тесты: `shared_array_buffer_hidden_without_cross_origin_isolation` /
  `shared_array_buffer_visible_when_cross_origin_isolated` /
  `worker_scope_hides_shared_array_buffer`; WebGL — `get_parameter_lives_on_prototype`,
  `context_instance_has_no_own_members`, `prototype_method_rejects_foreign_this`,
  `webgl2_prototype_has_own_constants`, `canvas_attrs_are_prototype_accessors`;
  Crypto — `crypto_subtle_have_prototype_backed_interfaces`,
  `crypto_constructor_is_illegal`; RTC — `rtcpeerconnection_interface_shape`
  (конструирование, состояние, honest `NotSupportedError` sync+async,
  illegal invocation), `rtcpeerconnection_absent_on_insecure_origin`.
- `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чисто.
- `python graphic_tests/dump_golden.py` — 12/12 PASS.
- `scripts/scoped-test.sh` — прогон остановлен на известном env-зависании:
  5 DOM-quota perf-тестов `v8_perf_typedom_node` (`dom_create_*_throws_quota_exceeded_when_full`,
  `dom_node_count_at_max_after_prefill`) висят на заполнении 50k нод под
  нагрузкой — код не тронут, тот же симптом зафиксирован в гейте WS-C. Прогон
  доведён вручную: lumen-js `--lib` 5167 зелёных (5 падений — 1 реальный
  порядок маркеров splice-теста, исправлен; 4 тайминговых флака — зелёные
  в изоляции), `tests/all.rs` 61, `chrome_keyboard_owner`, `popup_gate` — ok;
  пакеты после lumen-js (knowledge…update-manifest) прогнаны отдельно.
  Остаточные падения — предсуществующие/средовые, к диффу отношения не имеют:
  `snapshot_cpu` (флаки), `hevc_hardware_matches_software` (GPU/CPU пиксели),
  5 `cases::snapshot_tests` + `empty_cells_hide_ignored` (env-ширина текста),
  `css_pending::deadline_releases_pending_once` (тайминг).
- Живая проба `probe_native_api.py` не перепрогонялась: `.tmp/stall/probe_native.py`
  был scratch-артефактом и удалён, а закоммиченный пробник меряет
  `toString`-классификацию, не форму прототипов — четыре пункта этого бага она
  напрямую не показывает. Верификация — V8-рантайм тесты выше (реальные
  `install_dom`/worker-скоупы).

## Остаток / ограничения

- Воркеры не наследуют `crossOriginIsolated` от creator'а (п COOP/COEP →
  worker install не заведён): на изолированной странице воркер без SAB —
  расхождение с Chrome, зафиксировано в комментарии `sab_gate_shim.js`.
- `RTCPeerConnection` — детектируемый интерфейс с честными фейлами, не
  функциональный WebRTC; ICE-кандидатный фингерпринт по-прежнему пуст.
  Если понадобится поведение — отдельная задача (нативный бэкенд).
- Строки `UNMASKED_VENDOR/RENDERER` остаются синтетическими `GpuFingerprint`
  — отдельный вопрос, отмечен в исходной записи.
