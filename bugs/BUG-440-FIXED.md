# BUG-440 — на `file://` нативная GET-отправка формы строит непригодный путь: query-string попадает в имя файла

**Статус:** FIXED 2026-08-30
**Компонент:** shell (`crates/shell/src/resource_base.rs` — `ResourceBase::resolve`/`resolve_str` для варианта `ResourceBase::File`, вызов из ветки `FormClickAction::SubmitForm`, метод `"get"`)
**Найден:** 2026-07-29, P1, регресс-прогон при починке [BUG-437](BUG-437-FIXED.md)
**Починил:** P3, 2026-08-30

## Симптом

Страница открыта как `file:///…/form.html`, форма `method="get"
action="target.html"` с полем `q=hello`. Клик по submit-кнопке даёт в
`activity.log`:

```
NAV        → "D:/RustProjects/…/.tmp\form437target.html?q=hello"
LOAD_START "D:/RustProjects/…/.tmp\form437target.html?q=hello"
LOAD_ERR   … — Синтаксическая ошибка в имени файла, имени папки или метке тома. (os error 123)
```

То есть отправка выполняется, но целевая страница не грузится никогда.

Тот же путь ломает и `action` со схемой: `action="about:blank"` превращается в
`"D:/…/.tmp\about:blank"` и падает с os error 2.

## Причина

`forms::make_get_url` приклеивает `?<urlencoded>` к `action`, после чего
`PageSource::resolve_href` → `ResourceBase::resolve_str`. Для страницы,
загруженной с диска, база — `ResourceBase::File(path)`, и `resolve` соединяет
родительский каталог с href **как с путём файловой системы**: `?q=hello`
остаётся частью имени файла, разделитель берётся платформенный (`\` вперемешку
с `/`), а абсолютные URL с чужой схемой (`about:`, `http:`) не распознаются и
тоже приклеиваются к каталогу.

## Ожидалось

Для `file://`-базы GET-отправка должна отделять query-string от пути (файл
читается по пути, `?…` в имя не входит), а href с собственной схемой —
резолвиться как URL, а не как относительный путь.

## Починено

`ResourceBase::resolve` для варианта `File`: href с собственной схемой
(`about:`, `data:`, `mailto:`, ...) больше не приклеивается к каталогу —
резолвится через новый `href_scheme` (RFC 3986 §3.1, однобуквенная «схема» не
считается — иначе диск `D:` читался бы как чужая схема) в `ResolvedResource::Url`.
Для `?query`/`#fragment` — путь ФС не может их нести, поэтому они отрезаются
до `dir.join(...)` новым `url_path_component`, который заодно декодирует
percent-escape (`my%20file.html` → `my file.html`, как и должно быть при чтении
файла по URL-ссылке); пустой остаток (`"?q=1"`, `"#sec"`) резолвится в тот же
файл (base_path), а не в бессмысленный `dir.join("")`.

Отдельно найден и починен тот же дефект на схеме `file:`: раньше
`file:///D:/other/x.html` смешивался с путём как посторонняя строка (тот же
симптом, что и `about:blank`); теперь резолвится в реальный путь через общий
`resource_base::file_url_to_path`, которым переиспользован и
`page_source_for_automation_url` (BiDi/MCP-навигация) — раньше та же логика
была продублирована там без обрезки query/fragment и без percent-decode.

## Смежное

- [BUG-346](BUG-346-FIXED.md) — `Url::resolve()` не схлопывала `..` (тот же слой резолва URL, другой дефект; уже починен).
- [BUG-437](BUG-437-FIXED.md) — разбор пути клик → отправка формы, в ходе которого это найдено.
