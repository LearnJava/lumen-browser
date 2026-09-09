//! E2E-1: инварианты POST-навигации на уровне `PageSource`.
//!
//! Сам сетевой обмен проверяется в `lumen-network`
//! (`tests/cases/nav_post_body.rs` — метод, `Content-Type`, тело, 302 → GET).
//! Здесь — вторая половина задачи: тело живёт ровно одну загрузку и не
//! просачивается ни в историю, ни в снимок сессии, ни в bfcache.

use super::*;

/// Обычная навигация тела не несёт — `PageSource::url` для того и заведён,
/// чтобы ни один из десятка call-site-ов (адресная строка, история, вкладки,
/// автоматизация) не мог случайно отправить POST.
#[test]
fn plain_url_source_carries_no_body() {
    let src = PageSource::url("https://example.com/login");
    assert!(src.nav_body().is_none());
}

/// Источник с телом остаётся во всём остальном обычной сетевой страницей:
/// адрес, origin хранилищ и база подресурсов считаются от него ровно так же.
/// Ради этого тело — поле варианта `Url`, а не отдельный вариант enum-а:
/// отдельный вариант пришлось бы дописывать в каждый из этих match-ей, а
/// там, где стоит `_ =>`, он бы молча получил чужое поведение.
#[test]
fn post_source_behaves_like_a_url_source() {
    let src = PageSource::Url {
        url: "https://example.com/login".to_owned(),
        body: Some(Box::new(lumen_network::NavigationBody::post(
            "application/x-www-form-urlencoded",
            b"user=admin".to_vec(),
        ))),
    };
    assert_eq!(src.url_str(), Some("https://example.com/login"));
    assert_eq!(src.describe(), "https://example.com/login");
    assert_eq!(src.origin_str().as_deref(), Some("https://example.com"));
    // `ResourceBase` не выводит ни `Debug`, ни `PartialEq` — сравниваем через
    // `matches!`, а не через `assert_eq!`.
    assert!(matches!(
        src.resource_base(),
        Some(ResourceBase::Url(ref u)) if u == "https://example.com/login"
    ));
    // И тело при этом на месте — иначе тест выше проверял бы пустоту.
    let body = src.nav_body().expect("body");
    assert_eq!(body.method, "POST");
    assert_eq!(body.bytes, b"user=admin");
}

/// `forget_nav_body` — то, чем `reload()` делает тело одноразовым: адрес
/// остаётся, тело исчезает. Всё, что читает `self.source` ПОСЛЕ загрузки —
/// запись в `nav_back`, снимок сессии, F5, клон вкладки — видит уже GET.
#[test]
fn forget_nav_body_keeps_url_and_drops_body() {
    let mut src = PageSource::Url {
        url: "https://example.com/login".to_owned(),
        body: Some(Box::new(lumen_network::NavigationBody::post(
            "application/x-www-form-urlencoded",
            b"user=admin".to_vec(),
        ))),
    };
    src.forget_nav_body();
    assert!(src.nav_body().is_none(), "тело пережило загрузку — F5 ре-постнёт форму");
    assert_eq!(src.url_str(), Some("https://example.com/login"));
    // Повторный вызов безопасен и ничего не ломает (reload может пройти
    // обеими ветками — streaming и синхронной — на разных навигациях).
    src.forget_nav_body();
    assert_eq!(src.url_str(), Some("https://example.com/login"));
}

/// Снимок сессии хранит только адрес: тело не попадает на диск даже если бы
/// `forget_nav_body` кто-то не вызвал. Пароль из формы входа переживать
/// перезапуск браузера не должен.
#[test]
fn session_snapshot_of_post_source_is_url_only() {
    let src = PageSource::Url {
        url: "https://example.com/login".to_owned(),
        body: Some(Box::new(lumen_network::NavigationBody::post(
            "application/x-www-form-urlencoded",
            b"pass=secret".to_vec(),
        ))),
    };
    assert_eq!(
        session_persist::source_url_string(&src),
        Some("https://example.com/login".to_owned())
    );
}
