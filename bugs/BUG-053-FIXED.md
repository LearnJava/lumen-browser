# BUG-053

**Статус:** FIXED 2026-06-02
**Компонент:** shell
**Файл:** `crates/shell/src/main.rs:927,1051,3112`

## Описание

`cargo build -p lumen-shell --features quickjs` не компилировался: trait PersistentJs не объявлял update_scroll_states/take_scroll_requests (merge p1-js-scroll-drain/p1-clickable-iterator потерял декларации в trait+impl при разрешении конфликта), а call-site в relayout() брал self иммутабельно (js+lb_ref) и тут же звал self.fetch_and_register_lazy_images(&mut self) → E0502. Default-gate (без quickjs) собирался, поэтому регрессия не ловилась. Fix: восстановил декларации+forwarding методов в trait/impl, вынес lazy fetch за пределы иммутабельного borrow. Восстановлено при работе над задачей #26 (clipboard) — feature gated на quickjs, иначе не верифицируема
