# Shell UI Architecture — Panel/Surface System

**Decision:** [ADR-009](decisions/ADR-009-shell-panel-system.md)  
**Status:** Specification (not yet implemented)  
**Phase:** Phase 1 target  
**Cross-platform:** Windows · macOS · Linux (winit + wgpu, zero OS-specific code in panel layer)

---

## 1. Overview

The browser shell UI is built from **panels**. A panel is any self-contained visual
block: a tab tree, an address bar, a bookmark popover, a picture-in-picture video
window, a privacy dashboard. Every panel implements one trait — `Panel` — and declares
one value — `Surface` — that tells the system where and how to show it.

The system has five layers:

```
┌─────────────────────────────────────────────────────────┐
│  AppState  — единственный источник данных               │
│  (профили, воркспейсы, вкладки, закладки, история…)    │
├─────────────────────────────────────────────────────────┤
│  SurfaceManager  — координатор                          │
│  (layout tree, routing событий, выполнение команд)     │
├───────────────┬─────────────────┬───────────────────────┤
│  Layout Tree  │   Float Layer   │   OsWindow Registry   │
│  (docked      │   (overlays,    │   (PiP, DevTools,     │
│   panels)     │    popups,      │    music player,      │
│               │    notifications│    separate windows)  │
├───────────────┴─────────────────┴───────────────────────┤
│  Panel  — интерфейс каждого UI-блока                    │
│  paint() → DisplayList → wgpu (один pass на весь экран)│
└─────────────────────────────────────────────────────────┘
```

**Ключевой принцип:** панель знает только себя. Она не знает где на экране находится,
кто её соседи, сколько других панелей существует. Она получает прямоугольник и рисует
себя в нём.

---

## 2. Кросс-платформенность

Весь panel-код не содержит `#[cfg(target_os = ...)]`. Платформенные различия
изолированы в двух местах:

| Слой | За что отвечает | Кросс-платформенность |
|---|---|---|
| `winit` | Окна ОС, события, курсор, IME, DnD | Автоматически: Win/macOS/Linux |
| `wgpu` | GPU рендеринг | DX12 / Metal / Vulkan / WebGPU |
| `lumen-shell::platform` | Уведомления ОС, always-on-top нюансы, системные шрифты | Один trait, три impl |
| Panel код | Paint + события + логика | Нет OS-зависимостей |

```
                  Panel::paint() → DisplayList
                         │
                         ▼
              lumen-paint → wgpu Surface
              [работает на всех платформах]

         OsNotification → platform::notify()
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
           Windows    macOS      Linux
          (WinRT)   (UNUser-   (libnotify
                    Notif)      / D-Bus)
```

### Платформенные шрифты (автоматически через Theme)

```rust
impl Theme {
    pub fn system_ui_font() -> FontId {
        #[cfg(target_os = "macos")]    { FontId::SystemSF }
        #[cfg(target_os = "windows")]  { FontId::SegoeUI }
        #[cfg(target_os = "linux")]    { FontId::SystemSans }
    }
    pub fn system_mono_font() -> FontId {
        #[cfg(target_os = "macos")]    { FontId::SFMono }
        #[cfg(target_os = "windows")]  { FontId::CascadiaCode }
        #[cfg(target_os = "linux")]    { FontId::SystemMono }
    }
}
```

Все `#[cfg]` — только здесь, нигде больше.

---

## 3. Интерфейс `Panel`

Девять методов. Каждый отвечает за строго одну вещь.

```rust
/// Каждый UI-блок реализует этот трейт.
/// Нет зависимостей от ОС. Нет глобального состояния.
/// Нет связей с другими панелями.
pub trait Panel: Send + 'static {

    // ── ИДЕНТИФИКАЦИЯ ─────────────────────────────────────────────

    /// Уникальное имя панели в системе.
    /// Используется для show/hide/close по имени.
    /// Примеры: "tab-tree", "address-bar", "bookmark-popover",
    ///          "pip-player", "command-palette", "privacy-panel"
    fn id(&self) -> &'static str;

    // ── РАСПОЛОЖЕНИЕ И РАЗМЕР ─────────────────────────────────────

    /// Где и как панель появляется.
    /// Docked / Float / OsWindow / Modal — см. Surface ниже.
    fn surface(&self) -> Surface;

    /// Желаемая ширина. SurfaceManager учитывает, но может скорректировать.
    fn width(&self) -> SizeRule;

    /// Желаемая высота.
    fn height(&self) -> SizeRule;

    // ── РИСОВАНИЕ ────────────────────────────────────────────────

    /// Нарисуй себя. Получаешь контекст с прямоугольником, темой,
    /// состоянием приложения. Возвращаешь список команд рисования.
    ///
    /// Вызывается только когда панель помечена dirty.
    /// НЕ вызывается каждый кадр — только при изменении состояния.
    fn paint(&self, ctx: &PaintCtx) -> DisplayList;

    // ── HIT-TEST ─────────────────────────────────────────────────

    /// Что находится под точкой pos (в координатах панели, не экрана)?
    /// None — пустое место, не реагирует.
    /// Some(HitTarget) — конкретный элемент: кнопка, ссылка, вкладка…
    fn hit_test(&self, pos: Point) -> Option<HitTarget>;

    // ── СОБЫТИЯ ──────────────────────────────────────────────────

    /// Обработай событие. Верни что делать дальше:
    /// Consumed / Ignored / Command(…) / Close
    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx)
        -> EventResponse;

    // ── ФОКУС КЛАВИАТУРЫ ─────────────────────────────────────────

    /// Хочет ли эта панель получать ввод с клавиатуры?
    /// true — у address bar, command palette, текстовых полей.
    /// false — у большинства панелей по умолчанию.
    fn accepts_focus(&self) -> bool { false }

    // ── ЖИЗНЕННЫЙ ЦИКЛ ───────────────────────────────────────────

    /// Панель только что добавлена в систему.
    /// Можно загрузить начальные данные, запустить анимацию появления.
    fn on_mount(&mut self, _ctx: &mut EventCtx) {}

    /// Панель закрывается.
    /// Можно сохранить состояние, остановить таймеры.
    fn on_unmount(&mut self) {}

    /// Изменился размер окна или слота.
    fn on_resize(&mut self, _new_rect: Rect) {}

    /// Панель получила фокус клавиатуры.
    fn on_focus(&mut self) {}

    /// Панель потеряла фокус клавиатуры.
    fn on_blur(&mut self) {}
}
```

---

## 4. `Surface` — где и как появляется панель

```rust
pub enum Surface {

    /// ── В ДЕРЕВЕ LAYOUT (прикреплена к слоту) ───────────────────
    ///
    /// Панель занимает постоянное место в layout дереве.
    /// Размер вычисляется SurfaceManager из width()/height() и
    /// доступного пространства.
    ///
    /// Примеры: TabTree, WorkspaceBar, PrivacyPanel, DevToolsPanel
    Docked {
        slot: SlotId,    // имя слота: "left", "right", "bottom", "top"
    },

    /// ── ПЛАВАЕТ ВНУТРИ ГЛАВНОГО ОКНА ────────────────────────────
    ///
    /// Рисуется поверх layout на Float Layer.
    /// Не занимает место в layout дереве.
    /// Остаётся в пределах окна.
    ///
    /// Примеры: popover закладки, command palette, контекстное меню,
    ///          мини-плеер в углу, уведомление, карта, tooltip
    Float {
        anchor: FloatAnchor,
        /// Чем больше — тем выше. Tooltip(1000) поверх Menu(500).
        z_order: i32,
        /// Закрыть при клике за пределами панели?
        close_on_outside_click: bool,
    },

    /// ── ОТДЕЛЬНОЕ ОКНО ОС ────────────────────────────────────────
    ///
    /// Настоящее отдельное окно через winit.
    /// Можно вынести на другой монитор.
    /// Имеет свой Layout Tree и Float Layer.
    /// Разделяет AppState с главным окном.
    ///
    /// Примеры: Picture-in-Picture видео, музыкальный плеер,
    ///          DevTools в отдельном окне, менеджер загрузок,
    ///          отдельное окно для вкладки
    OsWindow {
        title: String,
        size: (u32, u32),
        min_size: Option<(u32, u32)>,
        max_size: Option<(u32, u32)>,
        /// Всегда поверх других окон (для PiP)
        always_on_top: bool,
        /// Рамка и кнопки ОС (false = без декораций, для PiP)
        decorations: bool,
        resizable: bool,
        /// Прозрачный фон (для кастомной формы окна)
        transparent: bool,
    },

    /// ── МОДАЛЬНЫЙ ДИАЛОГ ─────────────────────────────────────────
    ///
    /// Затемняет фон, блокирует ввод в другие панели.
    /// Центрируется в главном окне.
    ///
    /// Примеры: "Удалить профиль?", диалог печати, диалог разрешений
    Modal {
        /// Закрыть при клике на затемнённый фон?
        closable_on_backdrop: bool,
        /// Цвет затемнения (обычно rgba(0,0,0,0.5))
        backdrop_color: Color,
    },

    /// ── СИСТЕМНОЕ УВЕДОМЛЕНИЕ ────────────────────────────────────
    ///
    /// Через API операционной системы.
    /// Показывается даже если Lumen свёрнут.
    /// Реализация скрыта в lumen-shell::platform.
    ///
    /// Примеры: "Скачивание завершено", "Напоминание"
    OsNotification {
        title: String,
        body: String,
        /// None = постоянное, Some(ms) = исчезает через N мс
        timeout_ms: Option<u32>,
    },
}
```

### `FloatAnchor` — где появляется Float-панель

```rust
pub enum FloatAnchor {
    /// Рядом с курсором мыши.
    /// Для контекстных меню, tooltip.
    Cursor,

    /// Под конкретным прямоугольником (в экранных координатах).
    /// Для dropdown, popover кнопки.
    /// Автоматически flip если не помещается снизу — появится сверху.
    Below(Rect),

    /// Над прямоугольником.
    Above(Rect),

    /// В углу главного окна.
    /// Для мини-плеера, уведомлений, индикаторов.
    Corner(Corner),  // TopLeft / TopRight / BottomLeft / BottomRight

    /// По центру главного окна.
    /// Для command palette, поиска.
    Center,

    /// Точные координаты (экранные).
    Absolute(Point),

    /// Привязан к другой панели по ID.
    /// Перемещается вместе с ней при resize.
    AnchoredTo { panel: &'static str, side: Side },
}
```

### `SizeRule` — как панель описывает свой размер

```rust
pub enum SizeRule {
    /// Ровно N пикселей. Не сжимается, не растягивается.
    Fixed(f32),

    /// Занять всё доступное пространство.
    Flex,

    /// По размеру содержимого. SurfaceManager вызовет content_size().
    Content,

    /// Минимум min, максимум max, по умолчанию default.
    Range { min: f32, max: f32, default: f32 },

    /// Скрыт (0px). Панель существует, но не видна.
    /// Используется для условно видимых элементов.
    Hidden,
}
```

---

## 5. `PaintCtx` — контекст для рисования

Панель получает его в `paint()`. Содержит всё необходимое для рисования.
Ничего лишнего — нет доступа к другим панелям, нет изменения состояния.

```rust
pub struct PaintCtx<'a> {
    /// Прямоугольник панели в экранных координатах.
    /// Рисуй только внутри него.
    pub rect: Rect,

    /// Тема: цвета, размеры, шрифты.
    pub theme: &'a Theme,

    /// Текущее состояние приложения (только чтение).
    pub state: &'a AppState,

    /// Масштаб дисплея (1.0 = обычный, 2.0 = Retina/HiDPI).
    /// Используй для рисования чётких линий на HiDPI.
    pub scale: f32,

    /// Панель сейчас имеет фокус клавиатуры?
    pub focused: bool,

    /// Текущая позиция курсора в координатах панели.
    /// None если курсор не над этой панелью.
    pub cursor_pos: Option<Point>,

    /// Что под курсором (результат последнего hit_test).
    pub hovered: Option<HitTarget>,

    /// Прошедшее время с предыдущего кадра (для анимаций).
    pub dt: Duration,
}
```

---

## 6. `PanelEvent` — события которые получает панель

```rust
pub enum PanelEvent {
    // ── МЫШЬ ────────────────────────────────────────────────────
    /// Курсор вошёл в область панели.
    MouseEnter,
    /// Курсор покинул область панели.
    MouseLeave,
    /// Курсор переместился. pos — в координатах панели.
    MouseMove { pos: Point },
    /// Кнопка мыши нажата.
    MouseDown { pos: Point, button: MouseButton },
    /// Кнопка мыши отпущена.
    MouseUp   { pos: Point, button: MouseButton },
    /// Клик (down + up в той же области).
    Click     { pos: Point, button: MouseButton },
    /// Правый клик (контекстное меню).
    RightClick { pos: Point },
    /// Двойной клик.
    DoubleClick { pos: Point },
    /// Колёсико мыши / тачпад.
    Scroll    { delta: ScrollDelta },
    /// Перетаскивание (мышь зажата и движется).
    Drag      { from: Point, to: Point, button: MouseButton },

    // ── DRAG & DROP ──────────────────────────────────────────────
    /// Что-то перетащили в область панели.
    DragEnter { data: DragData },
    /// Перетаскивание ушло из области.
    DragLeave,
    /// Отпустили перетаскиваемый объект.
    Drop      { data: DragData, pos: Point },

    // ── КЛАВИАТУРА (только у панели с фокусом) ───────────────────
    /// Клавиша нажата.
    KeyDown   { key: Key, mods: Modifiers },
    /// Клавиша отпущена.
    KeyUp     { key: Key, mods: Modifiers },
    /// Текстовый ввод (с учётом IME, раскладки, мёртвых клавиш).
    /// Используй для ввода символов, не для хоткеев.
    TextInput { text: String },
    /// IME: промежуточный ввод (подсветить, не коммитить).
    ImeCompose { text: String },
    /// IME: финальный коммит.
    ImeCommit  { text: String },

    // ── ФОКУС ───────────────────────────────────────────────────
    /// Панель получила фокус клавиатуры.
    FocusGained,
    /// Панель потеряла фокус клавиатуры.
    FocusLost,

    // ── ЖИЗНЕННЫЙ ЦИКЛ ───────────────────────────────────────────
    /// Панель добавлена в систему, rect уже известен.
    Mounted,
    /// Панель сейчас будет удалена.
    Unmounted,
    /// Изменился размер окна или слота.
    Resized { new_rect: Rect },

    // ── СОСТОЯНИЕ ПРИЛОЖЕНИЯ ─────────────────────────────────────
    /// Что-то изменилось в AppState. Панель решает сама — важно ли.
    StateChanged(StateChange),
    /// Сменилась тема.
    ThemeChanged,
}

/// Что именно изменилось (чтобы панели не перерисовывались зря)
#[derive(Clone, Debug)]
pub enum StateChange {
    TabsChanged,
    BookmarksChanged,
    HistoryChanged,
    ProfileSwitched,
    WorkspaceSwitched,
    NavigationStarted { url: String },
    NavigationFinished { url: String, title: String },
    DownloadProgress  { id: DownloadId, bytes: u64, total: Option<u64> },
    DownloadFinished  { id: DownloadId, path: PathBuf },
    PrivacyEvent      { tab: TabId },
    FocusModeChanged  { active: bool },
    ReadingProgress   { tab: TabId, fraction: f32 },
    PomodoroTick      { remaining_secs: u32 },
    PomodoroComplete,
    SyncStatusChanged { synced_at: SystemTime },
    MemoryPressure    { level: MemoryPressureLevel },
}
```

---

## 7. `EventResponse` — что панель возвращает на событие

```rust
pub enum EventResponse {
    /// Я обработал событие. Не передавать дальше.
    Consumed,

    /// Я не обработал. Передать следующей панели или системе.
    Ignored,

    /// Выполни эту команду (изменение состояния, открытие окна…).
    Command(Command),

    /// Выполни несколько команд подряд.
    Commands(Vec<Command>),

    /// Закрой меня. (Эквивалент Command::CloseSurface(self.id()))
    Close,
}
```

---

## 8. `Command` — как панели меняют состояние

Панели **никогда не меняют AppState напрямую**. Только через `Command`.
`SurfaceManager` получает команду → обновляет `AppState` → уведомляет все панели через
`StateChanged`.

```rust
pub enum Command {

    // ── НАВИГАЦИЯ ────────────────────────────────────────────────
    Navigate      { url: String, new_tab: bool },
    GoBack,
    GoForward,
    Reload        { bypass_cache: bool },
    NavigateFragment { fragment: String },

    // ── ВКЛАДКИ ──────────────────────────────────────────────────
    NewTab        { workspace: Option<WorkspaceId>, url: Option<String> },
    CloseTab      (TabId),
    SelectTab     (TabId),
    PinTab        (TabId),
    UnpinTab      (TabId),
    HibernateTab  (TabId),
    WakeTab       (TabId),
    MoveTab       { tab: TabId, to_workspace: WorkspaceId },
    MoveTabToWindow { tab: TabId },            // открыть в новом OsWindow
    DuplicateTab  (TabId),
    SetTabProxy   { tab: TabId, proxy: Option<ProxyConfig> },
    SplitView     { tab_a: TabId, tab_b: TabId },
    CloseSplitView,

    // ── ВОРКСПЕЙСЫ ───────────────────────────────────────────────
    SwitchWorkspace (WorkspaceId),
    CreateWorkspace { name: String, color: Color, emoji: Option<char> },
    RenameWorkspace { id: WorkspaceId, name: String },
    DeleteWorkspace (WorkspaceId),
    SetWorkspaceProxy { workspace: WorkspaceId, proxy: Option<ProxyConfig> },

    // ── ПРОФИЛИ ──────────────────────────────────────────────────
    SwitchProfile   (ProfileId),
    CreateProfile   { name: String, color: Color },
    DeleteProfile   (ProfileId),

    // ── ЗАКЛАДКИ (Вариант A / C) ──────────────────────────────────
    AddBookmark {
        url: String, title: String,
        folder: Option<FolderId>, tags: Vec<String>,
    },
    RemoveBookmark  (BookmarkId),
    MoveBookmark    { bookmark: BookmarkId, to_folder: FolderId },
    RenameBookmark  { bookmark: BookmarkId, title: String },
    CreateFolder    { parent: Option<FolderId>, name: String },
    DeleteFolder    (FolderId),

    // ── READ-LATER ────────────────────────────────────────────────
    SaveToReadLater { url: String, title: String },
    MarkReadLaterDone  (ReadLaterId),
    DeleteReadLater    (ReadLaterId),

    // ── ЗАМЕТКИ И ПОДСВЕТКИ ──────────────────────────────────────
    AddHighlight {
        url: String, text: String, color: HighlightColor,
        page_position: f32,
    },
    RemoveHighlight  (HighlightId),
    AddNote          { url: String, text: String, anchor: NoteAnchor },
    UpdateNote       { id: NoteId, text: String },
    DeleteNote       (NoteId),

    // ── ИСТОРИЯ ──────────────────────────────────────────────────
    ClearHistory     { range: HistoryRange },
    DeleteHistoryEntry (HistoryId),

    // ── ЗАГРУЗКИ ─────────────────────────────────────────────────
    Download         { url: String, filename: Option<String> },
    CancelDownload   (DownloadId),
    OpenDownloadedFile (DownloadId),

    // ── РЕЖИМЫ ───────────────────────────────────────────────────
    EnterFocusMode,
    ExitFocusMode,
    StartPomodoro    { duration_secs: u32 },
    PausePomodoro,
    ResetPomodoro,
    SetReaderFont    { size: f32, family: ReaderFont },
    SetReaderTheme   (ReaderTheme),

    // ── UI — открыть/закрыть поверхности ─────────────────────────
    OpenSurface      (Box<dyn Panel>),
    CloseSurface     (&'static str),         // по id панели
    ToggleSurface    (Box<dyn Panel>),       // открыть если закрыта, закрыть если открыта
    FocusSurface     (&'static str),
    OpenPiP          { stream_url: String }, // Picture-in-Picture

    // ── ТЕМА И НАСТРОЙКИ ─────────────────────────────────────────
    SetTheme         (Theme),
    SetPrivacyPolicy (PrivacyPolicy),        // strict / balanced / off
    SetLanguage      (String),

    // ── СИСТЕМА ──────────────────────────────────────────────────
    SaveSession      { name: Option<String> },
    RestoreSession   (SessionId),
    Print,
    ZoomIn, ZoomOut, ZoomReset,
    CopyToClipboard  (String),
    RequestFocus     (&'static str),         // дать фокус панели по id
}
```

---

## 9. `HitTarget` — что под курсором

Возвращается из `hit_test()`. Используется для: курсора мыши, tooltip, статус-бара,
определения кликаемого элемента.

```rust
pub struct HitTarget {
    /// Семантический тип элемента.
    pub element: HitElement,

    /// Какой курсор показать (Default / Pointer / Text / Grab / …)
    pub cursor: CursorIcon,

    /// Текст tooltip (показывается через ~500 мс hover).
    pub tooltip: Option<String>,

    /// URL для строки статуса (показывается при hover на ссылку).
    pub status_url: Option<String>,
}

pub enum HitElement {
    /// Кнопка с именем (для hover-эффекта в paint).
    Button(&'static str),

    /// Ссылка (навигация или открытие вкладки).
    Link { url: String },

    /// Вкладка браузера.
    Tab(TabId),

    /// Закладка.
    Bookmark(BookmarkId),

    /// Папка закладок.
    Folder(FolderId),

    /// Элемент истории.
    HistoryEntry(HistoryId),

    /// Ручка изменения размера панели.
    ResizeHandle { panel: &'static str, axis: Axis },

    /// Ручка перетаскивания.
    DragHandle,

    /// Ползунок скроллбара.
    Scrollbar,

    /// Полоса прокрутки (трек).
    ScrollTrack { above_thumb: bool },

    /// Обычный текст (cursor: Text, можно выделить).
    Text,

    /// Пустое место (cursor: Default).
    Empty,

    /// Кастомный элемент (для уникальных фич конкретной панели).
    Custom(String),
}
```

---

## 10. `EventCtx` — что панель может сделать в on_event

```rust
pub struct EventCtx<'a> {
    /// Текущее состояние (только чтение — изменения только через dispatch)
    pub state: &'a AppState,

    /// Отправить команду
    pub fn dispatch(&mut self, cmd: Command);

    /// Запросить перерисовку себя (пометить dirty)
    pub fn request_repaint(&mut self);

    /// Запросить перерисовку конкретной панели
    pub fn repaint_panel(&mut self, id: &'static str);

    /// Изменить курсор
    pub fn set_cursor(&mut self, cursor: CursorIcon);

    /// Захватить фокус клавиатуры
    pub fn request_focus(&mut self);

    /// Отдать фокус
    pub fn release_focus(&mut self);

    /// Начать перетаскивание (Drag & Drop)
    pub fn start_drag(&mut self, data: DragData);

    /// Запустить анимацию (вызывает paint каждый кадр пока активна)
    pub fn start_animation(&mut self, id: &'static str);
    pub fn stop_animation (&mut self, id: &'static str);

    /// Получить прямоугольник другой панели (для позиционирования popover)
    pub fn panel_rect(&self, id: &'static str) -> Option<Rect>;
}
```

---

## 11. `Theme` — все токены дизайна

Менять дизайн = менять Theme. Ни одна панель не хардкодит цвет или размер.

```rust
pub struct Theme {
    pub name: &'static str,   // "sand-indigo", "graphite-amber", "minimalist"

    // ── ЦВЕТА ОБОЛОЧКИ ───────────────────────────────────────────
    pub chrome_bg:     Color, // фон сайдбаров и баров
    pub chrome_deep:   Color, // titlebar, вложенные фоны
    pub chrome_edge:   Color, // разделители, границы
    pub paper:         Color, // фон контентной области
    pub paper_2:       Color, // альтернативный фон (полосы, выделение)

    // ── ТЕКСТ ────────────────────────────────────────────────────
    pub ink:           Color, // основной текст
    pub ink_soft:      Color, // вторичный текст
    pub ink_mute:      Color, // неактивный текст
    pub ink_fade:      Color, // очень бледный (placeholder)

    // ── АКЦЕНТ ───────────────────────────────────────────────────
    pub accent:        Color, // основной акцент (кнопки, активные эл-ты)
    pub accent_soft:   Color, // мягкий фон акцента (выделенная строка)
    pub accent_line:   Color, // тонкая линия акцента

    // ── ВОРКСПЕЙСЫ ───────────────────────────────────────────────
    pub ws_colors: [Color; 4], // Work / Personal / Research / Guest

    // ── ПРИВАТНОСТЬ ──────────────────────────────────────────────
    pub privacy_ok:      Color, // зелёный — чисто
    pub privacy_warn:    Color, // оранжевый — есть third-party
    pub privacy_blocked: Color, // красный — трекеры заблокированы

    // ── СОСТОЯНИЯ ────────────────────────────────────────────────
    pub state_active:   Color, // активный элемент
    pub state_hover:    Color, // hover
    pub state_selected: Color, // выбранный (другой от active)
    pub state_disabled: Color, // неактивный

    // ── РАЗМЕРЫ ПАНЕЛЕЙ ───────────────────────────────────────────
    pub sidebar_w:    f32,  // 280px (V1) / 260px (V3) / 48px (V2 rail)
    pub topbar_h:     f32,  // 40px
    pub titlebar_h:   f32,  // 32px
    pub tab_row_h:    f32,  // 26px
    pub tab_indent:   f32,  // 16px — отступ дочерней вкладки
    pub right_panel_w:f32,  // 400px (V5 privacy) / 320px (V3 knowledge)

    // ── ТИПОГРАФИКА ──────────────────────────────────────────────
    pub font_ui:      FontSpec, // основной шрифт интерфейса
    pub font_mono:    FontSpec, // моноширинный
    pub font_serif:   FontSpec, // серифный (V3 Knowledge, V4 Focus)

    pub size_base:    f32,  // 13px
    pub size_small:   f32,  // 11px
    pub size_label:   f32,  // 10px (UPPERCASE labels)
    pub size_title:   f32,  // 15px

    // ── ФОРМА ────────────────────────────────────────────────────
    pub radius_sm:    f32,  // 3px
    pub radius_md:    f32,  // 6px
    pub radius_lg:    f32,  // 10px (window)

    // ── ТЕНИ ─────────────────────────────────────────────────────
    pub shadow_panel:  Shadow, // для popover, floating panels
    pub shadow_window: Shadow, // для OsWindow

    // ── АНИМАЦИИ ─────────────────────────────────────────────────
    pub anim_hover:    Duration, // 150ms — смена hover
    pub anim_panel:    Duration, // 250ms — появление панели
    pub anim_theme:    Duration, // 300ms — смена темы
}

/// Встроенные темы (из design файлов)
impl Theme {
    /// V1, Bookmarks A/B/C — тёплый песок + индиго
    pub fn sand_indigo() -> Self { ... }

    /// V2 — тёмный графит + янтарь
    pub fn graphite_amber() -> Self { ... }

    /// V3 — крем + терракота
    pub fn cream_terracotta() -> Self { ... }

    /// V4 — пергамент + янтарь
    pub fn parchment_amber() -> Self { ... }

    /// V5 — тёмный графит + синий/красный
    pub fn graphite_blue() -> Self { ... }

    /// Minimalist — системные цвета
    pub fn minimalist() -> Self { ... }

    /// Автоматически выбрать системную тему
    pub fn system_default() -> Self {
        if system_is_dark_mode() { Self::graphite_amber() }
        else                     { Self::sand_indigo() }
    }
}
```

---

## 12. `AppState` — единственный источник данных

Панели читают данные отсюда. Никакого локального мутабельного состояния для
данных приложения — только UI-состояние (что выделено, развёрнуто, hover).

```rust
pub struct AppState {
    // ── ПРОФИЛИ ──────────────────────────────────────────────────
    pub profiles:         ProfileStore,
    pub active_profile:   ProfileId,

    // ── ВОРКСПЕЙСЫ ───────────────────────────────────────────────
    pub workspaces:       WorkspaceStore,
    pub active_workspace: WorkspaceId,

    // ── ВКЛАДКИ ──────────────────────────────────────────────────
    pub tabs:             TabStore,   // дерево вкладок
    pub active_tab:       TabId,
    pub split_tab:        Option<TabId>,  // второй таб в split view

    // ── НАВИГАЦИЯ ────────────────────────────────────────────────
    pub loading:          bool,
    pub load_progress:    f32,  // 0.0 – 1.0
    pub security:         SecurityInfo,  // TLS, ECH, DoH, fingerprint

    // ── ЗАКЛАДКИ ─────────────────────────────────────────────────
    pub bookmarks:        BookmarkStore,
    pub current_bookmarked: bool,  // текущая страница в закладках?

    // ── READ-LATER ────────────────────────────────────────────────
    pub read_later:       ReadLaterStore,

    // ── ИСТОРИЯ ──────────────────────────────────────────────────
    pub history:          HistoryStore,

    // ── ЗАМЕТКИ И ПОДСВЕТКИ ──────────────────────────────────────
    pub notes:            NoteStore,
    pub highlights:       HighlightStore,

    // ── ЗАГРУЗКИ ─────────────────────────────────────────────────
    pub downloads:        DownloadStore,

    // ── ПРИВАТНОСТЬ ──────────────────────────────────────────────
    pub privacy:          PrivacyState,  // блокировки, политика, лог

    // ── РЕЖИМЫ ───────────────────────────────────────────────────
    pub focus_mode:       bool,
    pub pomodoro:         Option<PomodoroState>,
    pub reader_settings:  ReaderSettings,

    // ── СИНХРОНИЗАЦИЯ ────────────────────────────────────────────
    pub sync:             SyncState,

    // ── ТЕМА ─────────────────────────────────────────────────────
    pub theme:            Theme,
}
```

---

## 13. `SurfaceManager` — координатор всего

Это ядро системы. Владеет деревом слотов, всеми панелями, всеми OsWindow.

```rust
pub struct SurfaceManager {
    /// Дерево layout — как расположены Docked панели
    layout_tree: LayoutNode,

    /// Float панели (сортируются по z_order при рисовании)
    float_layer: Vec<FloatEntry>,

    /// Открытые OS окна
    os_windows: HashMap<WindowId, OsWindowContext>,

    /// Единое состояние приложения
    state: Arc<RwLock<AppState>>,

    /// Кэши DisplayList для каждой панели
    display_cache: HashMap<&'static str, CachedDl>,

    /// Какая панель имеет фокус клавиатуры
    focused_panel: Option<&'static str>,

    /// Текущий hover
    hovered: Option<(&'static str, HitTarget)>,
}

impl SurfaceManager {
    /// Добавить панель. Место определяется panel.surface().
    pub fn register(&mut self, panel: Box<dyn Panel>);

    /// Убрать панель по ID.
    pub fn unregister(&mut self, id: &'static str);

    /// Показать/скрыть.
    pub fn set_visible(&mut self, id: &'static str, visible: bool);

    /// Добавить новый слот в дерево layout.
    pub fn add_slot(&mut self, slot: SlotDef, position: SlotPosition);

    /// Обработать событие от winit.
    pub fn on_os_event(&mut self, window_id: WindowId, event: &WindowEvent);

    /// Выполнить команду → обновить AppState → уведомить панели.
    pub fn execute(&mut self, cmd: Command);

    /// Запросить полный рендер (все dirty панели → один DisplayList → wgpu).
    pub fn render(&mut self, renderer: &mut Renderer);
}
```

### Дерево слотов (Layout Tree)

```rust
pub enum LayoutNode {
    /// Разделяет пространство на части
    Split {
        direction: SplitDir,  // Horizontal / Vertical
        children:  Vec<LayoutNode>,
        sizes:     Vec<SizeRule>,
        /// Разделитель можно перетаскивать мышью?
        resizable: bool,
    },

    /// Именованное место для Docked панелей
    Slot {
        id:       SlotId,
        /// Все панели в слоте — первая отображается
        panels:   Vec<Box<dyn Panel>>,
        /// Показывать tabs для переключения между панелями?
        tabbed:   bool,
    },
}

/// Добавить слот можно в любом месте дерева:
pub struct SlotDef {
    pub id:       SlotId,
    pub size:     SizeRule,
    pub resizable: bool,
}
pub enum SlotPosition {
    Before(&'static str),   // перед существующим узлом
    After(&'static str),    // после
    InsideFirst(&'static str), // внутрь, первым
    InsideLast(&'static str),  // внутрь, последним
    NewRoot { direction: SplitDir }, // новый корень дерева
}
```

---

## 14. Retained mode — когда перерисовывается панель

Панель перерисовывается только когда нужно. Три условия:

```
1. Панель сама вызвала ctx.request_repaint()
   (при смене hover, при анимации, при получении данных)

2. SurfaceManager выполнил Command и послал StateChanged
   (панель решает в on_event(StateChanged) — важно ли это ей)

3. Изменился Theme
   (все панели перерисовываются)
```

Если ни одно условие не выполнено — DisplayList берётся из кэша. Wgpu-draw происходит
каждый кадр, но с закэшированным результатом (нет аллокаций, нет paint()).

---

## 15. Пример: как добавить новую панель

### Простая панель (статус загрузки)

```rust
// crates/shell/src/panels/download_bar.rs

pub struct DownloadBar;

impl Panel for DownloadBar {
    fn id(&self) -> &'static str { "download-bar" }

    fn surface(&self) -> Surface {
        Surface::Docked { slot: "bottom" }
    }

    fn width(&self)  -> SizeRule { SizeRule::Flex }
    fn height(&self) -> SizeRule { SizeRule::Fixed(32.0) }

    fn paint(&self, ctx: &PaintCtx) -> DisplayList {
        let mut dl = DisplayList::new();
        let dl_text = format!(
            "Загрузка: {} из {}",
            ctx.state.downloads.active_count(),
            ctx.state.downloads.total_bytes_fmt()
        );
        dl.push(FillRect  { rect: ctx.rect, color: ctx.theme.chrome_bg });
        dl.push(DrawText  { text: dl_text, pos: ctx.rect.left_center(),
                            color: ctx.theme.ink, font: ctx.theme.font_ui });
        dl
    }

    fn hit_test(&self, _pos: Point) -> Option<HitTarget> { None }

    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx)
        -> EventResponse
    {
        if let PanelEvent::StateChanged(StateChange::DownloadProgress { .. }) = event {
            ctx.request_repaint();
        }
        EventResponse::Ignored
    }
}

// Регистрация в main:
surface_manager.register(Box::new(DownloadBar));
// Всё. Ничего больше не нужно трогать.
```

### Плавающий мини-плеер

```rust
pub struct MiniPlayer { track: TrackInfo, paused: bool }

impl Panel for MiniPlayer {
    fn id(&self) -> &'static str { "mini-player" }

    fn surface(&self) -> Surface {
        Surface::Float {
            anchor: FloatAnchor::Corner(Corner::BottomRight),
            z_order: 50,
            close_on_outside_click: false,
        }
    }

    fn width(&self)  -> SizeRule { SizeRule::Fixed(280.0) }
    fn height(&self) -> SizeRule { SizeRule::Fixed(64.0) }

    fn paint(&self, ctx: &PaintCtx) -> DisplayList { /* ... */ }

    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx)
        -> EventResponse
    {
        if let PanelEvent::Click { pos, .. } = event {
            if self.pause_btn_rect().contains(*pos) {
                return EventResponse::Command(Command::TogglePlayback);
            }
        }
        EventResponse::Ignored
    }
}
```

### PiP видео (отдельное окно)

```rust
pub struct PipWindow { stream_url: String }

impl Panel for PipWindow {
    fn id(&self) -> &'static str { "pip-window" }

    fn surface(&self) -> Surface {
        Surface::OsWindow {
            title: String::new(),   // без заголовка
            size:  (320, 180),
            min_size: Some((160, 90)),
            max_size: None,
            always_on_top: true,
            decorations:   false,   // без рамки ОС
            resizable:     true,
            transparent:   false,
        }
    }

    fn width(&self)  -> SizeRule { SizeRule::Flex }
    fn height(&self) -> SizeRule { SizeRule::Flex }

    fn paint(&self, ctx: &PaintCtx) -> DisplayList { /* рендер видео */ }

    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx)
        -> EventResponse
    {
        if let PanelEvent::DoubleClick { .. } = event {
            return EventResponse::Close; // закрыть двойным кликом
        }
        EventResponse::Ignored
    }
}
```

---

## 16. Дизайн-варианты из файлов — как они описываются

Каждый дизайн = набор зарегистрированных панелей + тема.

```rust
pub fn build_v1_shell(state: Arc<RwLock<AppState>>) -> SurfaceManager {
    let mut sm = SurfaceManager::new(state, Theme::sand_indigo());

    // Дерево слотов: left 280px | content
    sm.set_layout(Split::horizontal([
        Slot::new("left",    Fixed(280.0)),
        Slot::new("content", Flex),
    ]));

    sm.register(Box::new(TitleBar::new()));         // slot: top (в дереве выше)
    sm.register(Box::new(TopBar::new()));            // slot: top
    sm.register(Box::new(WorkspaceBar::new()));      // slot: left
    sm.register(Box::new(TabTree::new()));           // slot: left
    sm.register(Box::new(WebContent::new()));        // slot: content
    sm
}

pub fn build_v5_shell(state: Arc<RwLock<AppState>>) -> SurfaceManager {
    let mut sm = SurfaceManager::new(state, Theme::graphite_blue());

    // left 48px | content | right 400px
    sm.set_layout(Split::horizontal([
        Slot::new("left",    Fixed(48.0)),
        Slot::new("content", Flex),
        Slot::new("right",   Fixed(400.0)),
    ]));

    sm.register(Box::new(TitleBar::new()));
    sm.register(Box::new(IconRail::new()));          // slot: left (узкий)
    sm.register(Box::new(TopBar::new()));            // slot: top
    sm.register(Box::new(PrivacyStatusBar::new()));  // slot: top (под TopBar)
    sm.register(Box::new(WebContent::new()));        // slot: content
    sm.register(Box::new(NetworkPanel::new()));      // slot: right
    sm
}

pub fn build_minimalist_shell(state: Arc<RwLock<AppState>>) -> SurfaceManager {
    let mut sm = SurfaceManager::new(state, Theme::minimalist());

    // Только vertical split: top / content
    sm.set_layout(Split::vertical([
        Slot::new("top",     Fixed(88.0)),  // titlebar + tabstrip + bar
        Slot::new("content", Flex),
    ]));

    sm.register(Box::new(TitleBar::new()));
    sm.register(Box::new(TabStrip::new()));          // горизонтальные вкладки
    sm.register(Box::new(TopBar::new()));
    sm.register(Box::new(WebContent::new()));
    sm
}
```

---

## 17. Путь к self-hosted chrome (Phase 3+)

В Phase 3 каждую Panel-реализацию можно заменить Lumen-rendered HTML-фреймом:

```rust
pub struct HtmlPanel {
    html_path: &'static str,   // "shell/sidebar.html"
    session:   BrowserSession, // рендерит через Lumen engine
}

impl Panel for HtmlPanel {
    fn id(&self) -> &'static str { "tab-tree" }

    fn paint(&self, ctx: &PaintCtx) -> DisplayList {
        // Lumen рендерит sidebar.html в прямоугольник ctx.rect
        self.session.render(ctx.rect, ctx.theme)
    }

    fn on_event(&mut self, event: &PanelEvent, ctx: &mut EventCtx)
        -> EventResponse
    {
        // События пробрасываются в DOM sidebar.html
        self.session.dispatch(event, ctx)
    }
}
```

`SurfaceManager` не знает разницы между Rust-реализацией панели и HTML-панелью.
Миграция прозрачная, по одной панели за раз.

---

## 18. Схема базы данных (хранилище)

Все данные — в SQLite (ADR-003). Миграции применяются при старте, новые фичи
добавляют только новые таблицы.

```sql
-- 001: профили и воркспейсы
CREATE TABLE profiles (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    color       TEXT NOT NULL,        -- hex: "#3b82f6"
    emoji       TEXT,
    created_at  INTEGER NOT NULL,
    synced_at   INTEGER
);

CREATE TABLE workspaces (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    name        TEXT NOT NULL,
    color       TEXT NOT NULL,
    emoji       TEXT,
    position    INTEGER NOT NULL,
    proxy_host  TEXT,                 -- SOCKS5/HTTP прокси по умолчанию
    proxy_port  INTEGER,
    proxy_type  TEXT                  -- "socks5" / "http" / "tor"
);

-- 002: вкладки
CREATE TABLE tabs (
    id              INTEGER PRIMARY KEY,
    workspace_id    INTEGER NOT NULL REFERENCES workspaces(id),
    parent_tab_id   INTEGER REFERENCES tabs(id),  -- дерево вкладок
    url             TEXT NOT NULL,
    title           TEXT,
    favicon_url     TEXT,
    position        INTEGER NOT NULL,
    is_pinned       INTEGER NOT NULL DEFAULT 0,
    is_hibernated   INTEGER NOT NULL DEFAULT 0,
    scroll_y        REAL,
    proxy_host      TEXT,             -- per-tab прокси (переопределяет workspace)
    proxy_port      INTEGER,
    proxy_type      TEXT,
    last_active_at  INTEGER NOT NULL,
    created_at      INTEGER NOT NULL
);

-- 003: история
CREATE TABLE history (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    url         TEXT NOT NULL,
    title       TEXT,
    visited_at  INTEGER NOT NULL,
    visit_count INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX history_profile_time ON history(profile_id, visited_at DESC);

-- 004: полнотекстовый поиск по истории
CREATE VIRTUAL TABLE history_fts USING fts5(
    url, title, content,
    content=history, content_rowid=id
);

-- 005: закладки (Variant A / C)
CREATE TABLE bookmark_folders (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    parent_id   INTEGER REFERENCES bookmark_folders(id),
    name        TEXT NOT NULL,
    position    INTEGER NOT NULL
);

CREATE TABLE bookmarks (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    folder_id   INTEGER REFERENCES bookmark_folders(id),
    url         TEXT NOT NULL,
    title       TEXT NOT NULL,
    tags        TEXT,                 -- JSON array: ["rust","async"]
    created_at  INTEGER NOT NULL,
    synced_at   INTEGER
);

-- 006: read-later
CREATE TABLE read_later (
    id                  INTEGER PRIMARY KEY,
    profile_id          INTEGER NOT NULL REFERENCES profiles(id),
    url                 TEXT NOT NULL,
    title               TEXT NOT NULL,
    offline_html        BLOB,         -- снимок страницы для оффлайн чтения
    reading_progress    REAL DEFAULT 0.0,   -- 0.0 – 1.0
    estimated_minutes   INTEGER,
    tags                TEXT,         -- JSON array
    created_at          INTEGER NOT NULL,
    last_opened_at      INTEGER
);

-- 007: заметки
CREATE TABLE notes (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    url         TEXT NOT NULL,
    text        TEXT NOT NULL,
    anchor      TEXT,                 -- JSON: позиция на странице
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 008: подсветки (highlights)
CREATE TABLE highlights (
    id              INTEGER PRIMARY KEY,
    profile_id      INTEGER NOT NULL REFERENCES profiles(id),
    url             TEXT NOT NULL,
    selected_text   TEXT NOT NULL,
    note            TEXT,
    color           TEXT NOT NULL,    -- "amber" / "blue" / "green" / "red"
    page_position   REAL,             -- 0.0 – 1.0 (прокрутка страницы)
    created_at      INTEGER NOT NULL
);

-- 009: лог приватности (живёт только текущую сессию, не синхронизируется)
CREATE TABLE privacy_log (
    id              INTEGER PRIMARY KEY,
    tab_id          INTEGER NOT NULL REFERENCES tabs(id),
    url             TEXT NOT NULL,
    method          TEXT,             -- "GET" / "POST" / …
    resource_type   TEXT,             -- "script" / "image" / "font" / …
    is_blocked      INTEGER NOT NULL DEFAULT 0,
    is_third_party  INTEGER NOT NULL DEFAULT 0,
    bytes           INTEGER,
    duration_ms     INTEGER,
    recorded_at     INTEGER NOT NULL
);

-- 010: сессии
CREATE TABLE sessions (
    id          INTEGER PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    name        TEXT,
    data        TEXT NOT NULL,        -- JSON снимок воркспейсов и вкладок
    created_at  INTEGER NOT NULL
);

-- 011: настройки
CREATE TABLE settings (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id),
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (profile_id, key)
);

-- 012: синхронизация
CREATE TABLE sync_state (
    profile_id      INTEGER PRIMARY KEY REFERENCES profiles(id),
    device_id       TEXT NOT NULL,
    last_sync_at    INTEGER,
    sync_token      TEXT
);
```

---

## 19. Контрольный список для новой панели

Добавляя любую новую панель, убедись:

- [ ] Реализован `Panel` trait (все 9 методов / defaults где нужно)
- [ ] `id()` уникален в системе
- [ ] `surface()` возвращает правильный тип
- [ ] `paint()` рисует только внутри `ctx.rect`
- [ ] `hit_test()` покрывает все интерактивные элементы
- [ ] `on_event()` обрабатывает нужные события и возвращает `Command` вместо прямой мутации
- [ ] `on_event(StateChanged)` вызывает `ctx.request_repaint()` только для нужных изменений
- [ ] Нет `#[cfg(target_os = ...)]` в коде панели
- [ ] Нет прямого доступа к SQLite — только через `AppState`
- [ ] Нет прямых вызовов `winit` / `wgpu` — только через `PaintCtx` / `EventCtx`
- [ ] Все цвета берутся из `ctx.theme`, не хардкодятся
- [ ] Все размеры берутся из `ctx.theme` или `ctx.rect`
