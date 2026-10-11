# BUG-1606 — Permissions Policy: у `HTMLIFrameElement` нет `permissionsPolicy`/`featurePolicy` (548 TypeError в `permissions-policy`)

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-7 срез 67, baseline `permissions-policy`)
**Область:** js (`crates/js/src/permissions_policy.rs` — ставит только `document.featurePolicy`/`document.permissionsPolicy`)

## Симптом

Хелпер `tests/wpt/permissions-policy/resources/permissions-policy.js:466` читает `frame.permissionsPolicy` у созданного `<iframe>` и вызывает `frame_policy.allowedFeatures()` (стр. 483/485). У `HTMLIFrameElement` свойства нет, получается `undefined`, и каждый такой подтест падает с `TypeError: Cannot read properties of undefined (reading 'allowedFeatures')`.

## Охват

Прогон `--update-expected --all --root permissions-policy --recursive`: 548 вхождений этого `TypeError` в логе. Наибольшие файлы: `permissions-policy-frame-policy-allowed-for-some.https.sub.html` (76 подтестов), `permissions-policy-header-policy-allowed-for-{all,self,some,wildcard,malformed-wildcard}.https.sub.html`, `permissions-policy-header-policy-declined.https.sub.html`.

## Как проверить

`python tests/wpt/run_report.py --all --root permissions-policy --recursive --processes 8` и поискать `allowedFeatures` в логе. Или `document.createElement('iframe').permissionsPolicy` в консоли: ожидается объект `PermissionsPolicy`, сейчас `undefined`.

## Замечание

Одного свойства мало: подтесты проверяют результат `allowedFeatures()` с учётом заголовка `Permissions-Policy`, атрибута `allow` и `sandbox`. Заголовок и атрибут сейчас движком не применяются (Phase 0: `allowsFeature` возвращает `true`), поэтому после добавления свойства часть подтестов перейдёт из `TypeError` в обычный `FAIL`.
