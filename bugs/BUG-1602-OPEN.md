# BUG-1602 — `will-change` не создаёт containing block и стекинг-контекст для `position`, `translate`, `contain`, `offset-path`, `backdrop-filter`, `z-index`, `opacity` на inline

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** layout (`crates/engine/layout/src/stacking.rs:244-255` — `is_stacking_will_change`; определение containing block для `position:absolute|fixed`)

## Симптом

CSS Will Change §2: если значение свойства создало бы containing block (для fixed/abspos) или стекинг-контекст, то и `will-change: <это свойство>` обязан. У нас работает для `transform`, `filter`, `perspective`; не работает для остальных.

## Проба

`.tmp/s28/w3.html`, `--dump-layout`, `getBoundingClientRect` потомка `position:fixed; left:10px; top:10px` в родителе `margin-left:100px; margin-top:100px` с указанным `will-change` (нулевой сдвиг = родитель не стал containing block):

| `will-change` | `fixed`-потомок | containing block |
|---|---|---|
| `transform`, `filter`, `perspective` (и контрольные `filter:blur(0)`, `contain:paint`) | (110, N) | да |
| `translate`, `contain`, `offset-path`, `backdrop-filter` | (10, 10) | **нет** |
| `position` + `position:absolute` потомок | (10, 10) | **нет** |

## Охват

WPT-RUN-14 S28: `css/css-will-change` — 53 id, 22 reftest `thick`: `will-change-abspos-cb-*` (3), `will-change-fixedpos-cb-*` (5), `will-change-fixpos-cb-*` (6), `will-change-stacking-context-*` (8: `opacity` на inline, `z-index`, `mask-image`, `view-transition-name`, `transform-style`, `backdrop-filter`, `translate`, `offset-path`); плюс `will-change-computed`/`inheritance`/`parsing/will-change-invalid` (4 id, 153 подтеста: `getComputedStyle().willChange` — `""`; 127 из 127 в `invalid`). Часть имён (`stacking-context-*`) — по именам, пробой containing block они не покрыты.

## Как проверить

`css/css-will-change/will-change-abspos-cb-001.html`, `will-change-fixpos-cb-translate-1.html`, `will-change-stacking-context-opacity-2.html`.
