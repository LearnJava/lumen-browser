# BUG-1841 — паника `BUG-986` после навигации: субъект `:hover`/`:active` пережил уходящий документ (portal.nextera.ru, клик «Войти»)

**Статус:** FIXED 2026-11-26 — субъекты указателя сбрасываются на каждой междокументной навигации и перепроверяются по раскладываемому документу; клик «Войти» на портале больше не роняет процесс.
**Заведён:** 2026-11-26 (ZCode, задача «portal.nextera.ru: падение по «Войти», зависание клика, отрисовка полей, клик по панели»)
**Компонент:** shell ([`navigation.rs`](../crates/shell/src/lumen/navigation.rs), [`relayout_job.rs`](../crates/shell/src/relayout_job.rs)), layout ([`style/matching/forms.rs`](../crates/engine/layout/src/style/matching/forms.rs))

## Симптом

`https://portal.nextera.ru/` (сборка `main` a0666c3564, dev-release): клик по «Войти» в левой панели
(`aside#ppSidePanel a.portal-login`) уходит на Keycloak — и процесс `lumen.exe` завершается с кодом 1.
В журнале — паника потока движка `BUG-986: NodeId … вне арены документа` в `style/matching/forms.rs`
на rayon-воркерах, затем отравленный `Mutex<Document>` и смерть потока `lumen-browser`.

Та же паника записана в [BUG-1218](BUG-1218-OPEN.md) на другом стенде (bankruptcy-platform) без найденного источника.

## Причина

При commit перехода сбрасывался только фокус (`focused_node`/`focused_frame`, BUG-995). `hovered_nid` и
`active_nid` в `Lumen` не сбрасывал никто: курсор за навигацию не двигался, `CursorMoved` не приходил, и индекс
узла уходящего документа уезжал в первый layout нового документа. `:hover`-матчинг доходил до `doc.get(node)`
(`is_self_or_ancestor`) с чужим индексом — на rayon-воркерах, под локом документа.

## Исправление

- `navigation.rs`: `reset_interactive_subjects()` — вызывается из каждой точки commit перехода вместо двух
  присваиваний фокуса; кроме фокуса сбрасывает `hovered_nid` (с эхом в движок `notify_hover_changed(None)`,
  как при смене hover в окне — BUG-1458) и `active_nid` (`sync_active_mirror`).
- `relayout_job.rs`: тройка `(hovered, focused, active)` снята на потоке браузера до постановки задания;
  для документа, который реально раскладывается, каждый индекс фильтруется по `contains_id`.
- `matching/forms.rs`: страховка `if !doc.contains_id(node) { return false; }` в `is_self_or_ancestor` —
  оставшийся индекс каскадирует как «нет соответствия», а не паникует.
- Тест `an_interactive_index_foreign_to_the_document_cascades_as_nothing_hovered`
  ([`pre_match_tests.rs`](../crates/engine/layout/src/style/tests/pre_match_tests.rs)).

## Проверка

- A/B настоящим кликом, 2026-11-25: сборка без правки — падение с кодом 1 и паникой `BUG-986`, с правкой —
  переход на страницу Keycloak.
- 2026-11-26, живое окно (`--mcp-live-port`): клик `a.portal-login` → `nextera.ru/auth-keycloak/…`, 67 узлов,
  `readyState complete`; затем вход пользователем настоящим вводом — портал открыт, паник в журнале 0.
- `cargo clippy -p lumen-shell -p lumen-layout --all-targets -- -D warnings` чисто; `scripts/scoped-test.sh`
  (2026-11-25): 21 825 прошло, 25 провалилось — все 25 совпали с базой на чистом `main` a0666c3564.

## Остаток

На стенде из BUG-1218 не перепроверялось: там падение недетерминированное (1 из 3 сессий). Совпадение симптома
(та же паника в том же файле после клиентских переходов с кликами по сайдбару) делает этот источник вероятным,
но измерения на том стенде нет.
