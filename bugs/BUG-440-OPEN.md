# BUG-440 — на `file://` нативная GET-отправка формы строит непригодный путь: query-string попадает в имя файла

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs` — `ResourceBase::resolve`/`resolve_str` для варианта `ResourceBase::File`, вызов из ветки `FormClickAction::SubmitForm`, метод `"get"`)
**Найден:** 2026-07-29, P1, регресс-прогон при починке [BUG-437](BUG-437-FIXED.md)

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

## Смежное

- [BUG-346](BUG-346-OPEN.md) — `Url::resolve()` не схлопывает `..` (тот же слой резолва URL, другой дефект).
- [BUG-437](BUG-437-FIXED.md) — разбор пути клик → отправка формы, в ходе которого это найдено.
