# BUG-1067 — form GET-отправка из `<iframe>` с `target="_top"`/`_parent` не апгрейжена и не несёт `Upgrade-Insecure-Requests`

**Статус:** OPEN
**Тип:** дефект реализованного кода — навигация страницы из этой ветки собрана в обход общего пути апгрейда/UIR-заголовка, которым уже пользуются соседние формы навигации того же фрейма.
**Заведён:** 2026-09-20 (P6, GAP-CSPENF срез 55 — попутная находка при добавлении UIR-заголовка для навигации `<iframe>`).
**Область:** `crates/shell/src/lumen/frame_form_submit.rs::frame_submit_navigate`, ветка `LinkTarget::Page`.
**Владелец:** P3 (P2/P6 не чинят чужие баги, кроме выданного пункта).

## Симптом

```rust
LinkTarget::Page => {
    ...
    let resolved = nav_base.resolve_str(get_url);
    self.navigate_to(PageSource::from_arg(Some(&resolved)));
}
```

Форма внутри `<iframe>` с `method="get"` и `target="_top"` (или `_parent` у фрейма глубины
0) резолвит `action` голым `ResourceBase::resolve_str` — без
`csp_enforce::upgrade_navigation_url` — и передаёт результат в `navigate_to` без
`PageSource::with_uir_header`. Если политика ребёнка несёт `upgrade-insecure-requests`,
итоговый запрос НЕ получает ни переписанной на `https:` схемы, ни заголовка
`Upgrade-Insecure-Requests: 1`.

Сосед по тому же файлу и функции — ветка `LinkTarget::Frame` (`self.navigate_frame_to(target_idx, get_url, nav_base, None)`) — этой проблемы не имеет: `navigate_frame_to` уходит в
`spawn_frame`, который сам зовёт `maybe_upgrade_frame_src`/вычисляет `send_uir_header` из
`csp_gate` хозяина цели (GAP-CSPENF срез 55). Гейт `form-action` (строками выше,
`form_action_blocked`) при этом отрабатывает верно для обеих веток — блокировка сама по
себе не сломана, проблема только в апгрейде/заголовке для ветки `Page`.

## Ожидание

Тот же приём, что `frame_links.rs::navigate_page_from_frame` уже применяет для ссылок
(GAP-CSPENF срезы 53/55): `resolved = crate::csp_enforce::upgrade_navigation_url(csp_gate, &nav_base.resolve_str(get_url))`, а `navigate_to` — с `.with_uir_header(crate::csp_enforce::navigation_wants_uir_header(csp_gate))`.

## Не проверено

- POST-ветка той же функции (`frame_form_submit.rs`, метод `post`) — не читалась при
  находке этого дефекта, возможно несёт тот же пробел.
- Живой замер (найдено чтением кода, не пробой).
