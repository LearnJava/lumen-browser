## 7. Web APIs

Реализуем по приоритету.

### Tier 1 (нужны для большинства сайтов)
- `document.*`, `Element.*`, `Node.*` — DOM API
- `querySelector`, `querySelectorAll`
- `addEventListener`, `removeEventListener`
- `fetch()`, `XMLHttpRequest`
- `localStorage`, `sessionStorage`
- `setTimeout`, `setInterval`, `requestAnimationFrame`
- `console.*`
- `window.location`, `window.history`
- `URL`, `URLSearchParams`
- `FormData` ✅, `Blob` ✅, `File` ✅, `FileReader` ✅, `btoa`/`atob` ✅
- `Promise` (даёт JS-движок)

### Tier 2
- `Canvas 2D`
- `IndexedDB`
- `WebSocket`
- `MutationObserver`, `IntersectionObserver`, `ResizeObserver`
- `requestIdleCallback`
- Clipboard API (read/write с разрешения)

### Tier 3 (опционально)
- Service Workers
- Web Workers
- Shadow DOM
- Custom Elements
- WebAssembly (через JS-движок «бесплатно»)

### Не реализуем
- WebUSB, WebBluetooth, WebMIDI, WebSerial, WebNFC, Payment Request, FedCM, WebHID, EME (DRM), Background Sync, Push, Notifications API (на старте).

**Crate:** `webapi/*`.

---

## 8. UI оболочка

### 8.1 Технологический выбор
- **`winit`** — окна, события.
- **`wgpu`** — рендеринг UI и engine compositor через один GPU-контекст.
- **`egui`** для v0.1–v1.0 — иммедиат-режим, очень быстро разрабатывается, кросс-платформенный.
- Возможный переход на `iced` или `Slint` к 2.0 для более polished UX.

### 8.2 Структура UI
```
┌──────────────────────────────────────────────────┐
│ [≡] [◀][▶][↻] [omnibox.................][⋯][↓]  │  toolbar
├────┬─────────────────────────────────────────────┤
│ ▾ Work             ┌─────────────────────────┐   │
│  ├ GitHub          │                         │   │
│  ├ Linear          │      Active tab         │   │
│  └ Docs            │      content area       │   │
│ ▾ Personal         │                         │   │
│  ├ HN              │                         │   │
│  └ Mail            └─────────────────────────┘   │
│ + New tab                                        │
├────┴─────────────────────────────────────────────┤
│ Network log: 12 req, 340 KB, 4 blocked          │  status bar
└──────────────────────────────────────────────────┘
```

### 8.3 Возможности UI

**Базовые:**
- Адресная строка (omnibox) с локальным поиском по истории/закладкам. Поисковые подсказки — **выключены по умолчанию**.
- Вкладки: вертикальные с деревьями (parent → children).
- Закладки: дерево, теги.
- История: полнотекстовый поиск по локальной БД.
- Find in page (Ctrl+F).
- Zoom (Ctrl+/Ctrl-).

**Продвинутые:**
- **Workspaces** — наборы вкладок, переключение Ctrl+1..9. Каждый — со своим контекстом cookies (опционально).
- **Tab tree** — вкладки иерархично, складываются по группам.
- **Tab hibernation** — фоновые вкладки выгружаются через N минут. Вкладка **остаётся видимой** в списке, активируется по клику.
- **Tab auto-archive** — отдельная семантика от hibernation: после N часов неактивности вкладка убирается из видимого списка во **внутренний archive**, остающийся доступным через `@history` / `@tabs`-поиск (§12.4) и FTS-индекс (§12.1). Запинованные вкладки не трогаются. Аналог `Today`-секции в Arc; решает «tab hoarding» без потери содержимого. Конкретный таймаут N — настройка пользователя, по умолчанию 12 ч.
- **Split view** — две вкладки рядом.
- **Picture-in-picture** для видео. ✅ Phase 0: in-window floating overlay (`Ctrl+Shift+V`, `shell/src/panels/pip_window.rs`); настоящее второе OS-окно ждёт multi-window рефактора.
- **Reader mode** (Ctrl+R) на основе `readability`.
- **Команд-палитра** Ctrl+Shift+P — все действия клавиатурой (как VS Code).
- **Network log панель** — что уходит, куда, сколько байт, что заблокировано.

**Темы:**
- Light, dark, system, AMOLED-black.
- Без анимаций по умолчанию (можно включить).
- Без округлых иконок 12-цветной палитры — функциональный минимум.

### 8.4 Чего НЕ делаем в UI
- Лент новостей, рекомендаций, шопинга, погоды.
- ИИ-сайдбара по умолчанию.
- Welcome-screens, туториалов, бейджей.
- «Вы давно не заходили» нотификаций.
- Forced sign-in.

---

