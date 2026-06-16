# BUG-167

**Статус:** OPEN
**Компонент:** shell

## Описание

При входе в полноэкранный режим через Fullscreen API (`element.requestFullscreen()` →
`window.set_fullscreen(Borderless)`) окно ОС растягивается на весь десктоп, но вьюпорт
страницы не пересчитывается под новые размеры fullscreen-окна.

Наблюдаемо: страница продолжает раскладываться в исходном вьюпорте (~1024×720),
`vw`/`vh` и `auto`-центрирование считаются от старых размеров — контент не растягивается
во весь экран, снизу/справа остаётся пустая область фона окна.

Ожидаемо (WHATWG Fullscreen): при `fullscreenchange` на вход вьюпорт должен принять
размеры fullscreen-области, страница — перелейаутиться, `window.innerWidth/innerHeight`
и `vw`/`vh` отразить новый размер.

## Как воспроизвести

1. Открыть любую страницу, вызвать `document.documentElement.requestFullscreen()`.
2. Окно уходит в borderless fullscreen на весь экран.
3. Контент остаётся в исходном вьюпорте, не растягивается.

## Подозрение

Шелл вызывает `w.set_fullscreen(Some(Borderless(None)))` (`crates/shell/src/main.rs:6400`),
но не прогоняет resize-обработчик: смена размера от set_fullscreen не доводится до
пересчёта viewport/relayout (тот же путь, что обычный `WindowEvent::Resized`).
Зеркальная проблема вероятна и на `exitFullscreen()`.
