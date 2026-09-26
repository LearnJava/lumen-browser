# BUG-1188 — `postMessage` из синхронного скрипта фрейма не доходит до родителя

**Статус:** OPEN
**Заведён:** 2026-09-26 (P6, найден при закрытии [BUG-648](BUG-648-FIXED.md), разбирая TIMEOUT
`performance-timeline/not-clonable.html`).
**Область:** js/shell — доставка сообщений между изолятами
([`crates/js/src/frame_bridge.rs`](../crates/js/src/frame_bridge.rs) `_lumen_f_post_message` и приёмный
конец `_lumen_deliver_frame_message`); точное место потери не локализовано.

## Симптом

Проба 2026-09-26 (dev-release, видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`): слушатель
`message` родителя зарегистрирован до `<iframe>`, ребёнок в своём первом синхронном `<script>` делает
`parent.postMessage("parent-early", "*")`, а через 200 мс `parent.postMessage("parent-t", "*")`.
Родитель получает только `parent-t`, `parent-early` пропадает. Chrome получает оба. Сообщения,
отправленные из `load` + `setTimeout(0)`, доходят.

## Что требуется

Сообщение, отправленное из фрейма в любой момент после создания его изолята, должно попасть в очередь
задач родителя (HTML LS «window post message steps»: задача ставится в очередь целевого окна, от
состояния загрузки отправителя не зависит). Критерий: проба выше отдаёт оба сообщения по порядку.
