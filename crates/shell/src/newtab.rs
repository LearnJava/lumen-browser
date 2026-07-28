//! `about:newtab` — статическая стартовая страница со «speed dial» из
//! закреплённых пользователем плиток + добивкой из истории
//! ([`lumen_storage::History::most_visited`]).
//!
//! HTML генерируется в памяти ([`build_newtab_html`]) и грузится как
//! [`PageSource::Static`][crate::PageSource] — без сетевого запроса. Сетка —
//! ровно [`MAX_TILES`] ячеек: закреплённые + добивочные сайты, и «+»-плитка,
//! если есть свободное место. Закрепление/открепление и «+» реализованы через
//! спец-ссылки `about:newtab?...` ([`NewtabAction`]/[`parse_action`]) — сама
//! страница не содержит JS, только `<a href>`.
//!
//! Phase 0 ограничения: favicon-картинки не загружаются — вместо них
//! буква-бейдж; страница статична: после открытия не реагирует на изменения
//! истории без перезагрузки (перезагрузка происходит автоматически при любом
//! действии над плиткой — см. `apply_newtab_action` в `main.rs`).

use lumen_core::form::{decode_form_value, encode_form_urlencoded, FormEntry};

/// Канонический URL стартовой страницы. Адресная строка и история показывают
/// именно эту строку.
pub const NEWTAB_URL: &str = "about:newtab";

/// Максимальное число ячеек в сетке speed dial (7 сайтов + «+»-плитка).
pub const MAX_TILES: usize = 8;

/// Число ячеек, доступных под реальные сайты — на одну меньше [`MAX_TILES`],
/// чтобы всегда оставалось место под «+», пока не закреплено все 8.
const MAX_SITE_TILES: usize = MAX_TILES - 1;

/// Одна плитка speed dial: целевой URL, заголовок и признак закрепления.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopSite {
    /// Абсолютный URL сайта (`href` плитки).
    pub url: String,
    /// Человекочитаемый заголовок (берётся из истории/пина; при пустом —
    /// подставляется хост URL вызывающей стороной).
    pub title: String,
    /// `true`, если это закреплённая пользователем плитка (store `NewtabTiles`),
    /// `false` — автоматическая добивка по посещаемости.
    pub pinned: bool,
}

/// Действие, закодированное в спец-ссылке `about:newtab?...`.
///
/// [`build_newtab_html`] генерирует эти ссылки на пин/анпин-переключателе
/// каждой плитки, на «+»-плитке и на кнопке «Восстановить закрытые».
/// [`parse_action`] восстанавливает действие из URL кликнутой ссылки — так
/// шелл может перехватить клик до попытки обычной навигации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewtabAction {
    /// Закрепить `url` (с заголовком `title`) новой плиткой.
    Pin { url: String, title: String },
    /// Открепить плитку с этим `url`.
    Unpin { url: String },
    /// «+»-плитка: закрепить страницу, с которой пользователь перешёл на newtab.
    PinCurrent,
    /// Кнопка «Восстановить закрытые»: заново открыть последнюю сохранённую сессию.
    RestoreClosed,
}

/// Разобрать резолвнутый URL кликнутой ссылки в [`NewtabAction`]; `None` —
/// если это не спец-ссылка (обычная плитка сайта или посторонний URL).
pub fn parse_action(url: &str) -> Option<NewtabAction> {
    let query = url.strip_prefix(NEWTAB_URL)?.strip_prefix('?')?;
    if query == "restore-closed" {
        return Some(NewtabAction::RestoreClosed);
    }
    if query == "pin-current" {
        return Some(NewtabAction::PinCurrent);
    }
    let mut pin_url = None;
    let mut title = None;
    let mut unpin_url = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        match k {
            "pin" => pin_url = Some(decode_form_value(v)),
            "title" => title = Some(decode_form_value(v)),
            "unpin" => unpin_url = Some(decode_form_value(v)),
            _ => {}
        }
    }
    if let Some(url) = unpin_url {
        return Some(NewtabAction::Unpin { url });
    }
    pin_url.map(|url| NewtabAction::Pin { url, title: title.unwrap_or_default() })
}

/// Построить спец-ссылку, закрепляющую `url`/`title` новой плиткой.
fn pin_link(url: &str, title: &str) -> String {
    format!(
        "{NEWTAB_URL}?{}",
        encode_form_urlencoded(&[FormEntry::text("pin", url), FormEntry::text("title", title)])
    )
}

/// Построить спец-ссылку, открепляющую плитку с этим `url`.
fn unpin_link(url: &str) -> String {
    format!("{NEWTAB_URL}?{}", encode_form_urlencoded(&[FormEntry::text("unpin", url)]))
}

/// Спец-ссылка «+»-плитки.
fn pin_current_link() -> String {
    format!("{NEWTAB_URL}?pin-current")
}

/// Спец-ссылка кнопки «Восстановить закрытые».
fn restore_closed_link() -> String {
    format!("{NEWTAB_URL}?restore-closed")
}

/// Слить закреплённые плитки (`pinned`, уже в порядке позиции из store) с
/// добивкой по посещаемости (`top_sites`), построив итоговый список ячеек
/// сетки. Закреплённые всегда идут первыми и в своём порядке; добивка
/// пропускает URL, уже присутствующие среди закреплённых.
///
/// Возвращает не более [`MAX_SITE_TILES`] элементов — если закреплено все
/// [`MAX_TILES`], возвращает все [`MAX_TILES`] (места под «+» больше нет).
/// «+»-плитку сюда не добавляет — это делает [`build_newtab_html`].
pub fn merge_tiles(pinned: &[TopSite], top_sites: &[TopSite]) -> Vec<TopSite> {
    let cap = if pinned.len() >= MAX_TILES { MAX_TILES } else { MAX_SITE_TILES };
    let mut out: Vec<TopSite> = pinned.iter().take(cap).cloned().collect();
    for site in top_sites {
        if out.len() >= cap {
            break;
        }
        if out.iter().any(|s| s.url == site.url) {
            continue;
        }
        out.push(site.clone());
    }
    out
}

/// Экранирует символы, опасные в HTML-тексте и в значении атрибута.
/// Применяется и к заголовкам, и к URL перед вставкой в разметку.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Первая буква бейджа плитки: первый алфавитно-цифровой символ хоста URL,
/// в верхнем регистре. Fallback — первый символ заголовка, затем `'?'`.
fn tile_initial(url: &str, title: &str) -> char {
    // Хост: часть между `://` и следующим `/`, `?` или `#`; иначе вся строка.
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // Отбрасываем `www.` и leading user-info при наличии.
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    host.chars()
        .find(|c| c.is_alphanumeric())
        .or_else(|| title.chars().find(|c| c.is_alphanumeric()))
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .unwrap_or('?')
}

/// Детерминированный цвет бейджа по букве — чтобы плитки визуально различались
/// без favicon. Палитра из 6 насыщенных тонов, индекс — по коду символа.
fn badge_color(initial: char) -> &'static str {
    const PALETTE: [&str; 6] = [
        "#3a6ff0", // синий
        "#e0533d", // красный
        "#2ca36b", // зелёный
        "#b657d6", // фиолетовый
        "#d99a1c", // янтарный
        "#1ca7b8", // бирюзовый
    ];
    PALETTE[(initial as usize) % PALETTE.len()]
}

/// Строит полный HTML страницы `about:newtab` со speed dial из `sites`
/// (уже слиты через [`merge_tiles`] и обрезаны до не более [`MAX_TILES`]).
///
/// Каждая плитка несёт пин/анпин-переключатель (спец-ссылка). Если после
/// `sites` остаётся свободная ячейка (меньше [`MAX_TILES`]), добавляется
/// «+»-плитка со спец-ссылкой [`pin_current_link`].
pub fn build_newtab_html(sites: &[TopSite]) -> String {
    let shown: Vec<&TopSite> = sites.iter().take(MAX_TILES).collect();
    let radius = crate::theme_tokens::radius::LG;

    let mut tiles = String::new();
    for site in &shown {
        let initial = tile_initial(&site.url, &site.title);
        let color = badge_color(initial);
        let href = escape_html(&site.url);
        let title = escape_html(&site.title);
        let (action_href, action_label, action_glyph) = if site.pinned {
            (escape_html(&unpin_link(&site.url)), "Открепить", "\u{2717}")
        } else {
            (escape_html(&pin_link(&site.url, &site.title)), "Закрепить", "\u{1F4CC}")
        };
        tiles.push_str(&format!(
            "<div class=\"tile\">\
               <a class=\"tile-body\" href=\"{href}\">\
                 <div class=\"badge\" style=\"background:{color}\">{initial}</div>\
                 <div class=\"name\">{title}</div>\
               </a>\
               <a class=\"pin\" href=\"{action_href}\" title=\"{action_label}\">{action_glyph}</a>\
             </div>"
        ));
    }
    if shown.len() < MAX_TILES {
        let add_href = escape_html(&pin_current_link());
        tiles.push_str(&format!(
            "<a class=\"tile add\" href=\"{add_href}\" title=\"Закрепить текущую страницу\">\
               <div class=\"badge add-badge\">+</div>\
               <div class=\"name\">Добавить</div>\
             </a>"
        ));
    }

    let restore_href = escape_html(&restore_closed_link());

    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\">\
<title>Новая вкладка</title><style>\
*{{box-sizing:border-box;}}\
body{{margin:0;background:#1b1b1f;color:#e8e8ea;\
font-family:'Inter',sans-serif;}}\
.wrap{{padding:120px 24px 24px 24px;text-align:center;}}\
h1{{font-size:30px;font-weight:600;margin:0 0 40px 0;color:#f4f4f6;}}\
.tiles{{display:flex;flex-wrap:wrap;justify-content:center;width:680px;\
margin:0 auto;}}\
.tile{{position:relative;display:block;width:150px;height:128px;margin:10px;}}\
.tile-body,.tile.add{{display:block;width:100%;height:100%;padding:18px 12px;\
background:#27272c;border-radius:{radius}px;text-decoration:none;\
text-align:center;box-sizing:border-box;}}\
.pin{{position:absolute;top:4px;right:4px;width:18px;height:18px;\
line-height:18px;font-size:10px;text-align:center;color:#9a9aa2;\
text-decoration:none;opacity:0.7;}}\
.pin:hover{{opacity:1;}}\
.badge{{width:56px;height:56px;margin:0 auto 14px auto;\
border-radius:{radius}px;color:#ffffff;font-size:22px;font-weight:600;\
line-height:56px;text-align:center;}}\
.add-badge{{background:#3a3a40;color:#c8c8ce;}}\
.name{{color:#d6d6da;font-size:14px;line-height:18px;overflow:hidden;}}\
.restore{{display:inline-flex;align-items:center;gap:6px;margin-top:8px;\
padding:6px 12px;border-radius:{radius}px;border:1px solid #3a3a40;\
color:#c8c8ce;text-decoration:none;font-size:13px;}}\
.restore:hover{{background:#27272c;}}\
</style></head><body><div class=\"wrap\"><h1>Lumen</h1>\
<div class=\"tiles\">{tiles}</div>\
<a class=\"restore\" href=\"{restore_href}\">Восстановить закрытые</a>\
</div></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(url: &str, title: &str) -> TopSite {
        TopSite { url: url.to_string(), title: title.to_string(), pinned: false }
    }

    fn pinned_site(url: &str, title: &str) -> TopSite {
        TopSite { url: url.to_string(), title: title.to_string(), pinned: true }
    }

    #[test]
    fn html_contains_site_url_and_title() {
        let html = build_newtab_html(&[site("https://example.com/", "Example")]);
        assert!(html.contains("href=\"https://example.com/\""), "{html}");
        assert!(html.contains("Example"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn html_escapes_special_chars_in_title_and_url() {
        let html = build_newtab_html(&[site(
            "https://x.test/?a=1&b=2",
            "<b>Tom & \"Jerry\"</b>",
        )]);
        // Сырые `<b>` / `&` не должны просочиться в разметку как теги/сущности.
        assert!(!html.contains("<b>Tom"));
        assert!(html.contains("&lt;b&gt;Tom &amp; &quot;Jerry&quot;&lt;/b&gt;"));
        assert!(html.contains("a=1&amp;b=2"));
    }

    #[test]
    fn html_limited_to_max_tiles() {
        let sites: Vec<TopSite> = (0..10)
            .map(|i| site(&format!("https://s{i}.test/"), &format!("Site {i}")))
            .collect();
        let html = build_newtab_html(&sites);
        // "tile add" не совпадает с точным `class="tile"`, так что здесь
        // считаются только реальные плитки-сайты.
        let tile_count = html.matches("class=\"tile\"").count();
        assert_eq!(tile_count, MAX_TILES);
        assert!(html.contains("https://s7.test/"));
        assert!(!html.contains("https://s8.test/"));
        // Сетка полностью занята реальными сайтами — «+» не показывается.
        assert!(!html.contains("class=\"tile add\""));
    }

    #[test]
    fn empty_sites_render_only_add_tile() {
        let html = build_newtab_html(&[]);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("</body></html>"));
        assert!(!html.contains("class=\"tile\""));
        assert!(html.contains("class=\"tile add\""));
    }

    #[test]
    fn pinned_tile_shows_unpin_link() {
        let html = build_newtab_html(&[pinned_site("https://a.test/", "A")]);
        assert!(html.contains("title=\"Открепить\""));
        assert!(!html.contains("title=\"Закрепить\""));
    }

    #[test]
    fn unpinned_tile_shows_pin_link() {
        let html = build_newtab_html(&[site("https://a.test/", "A")]);
        assert!(html.contains("title=\"Закрепить\""));
    }

    #[test]
    fn html_shows_plus_tile_hint_when_room() {
        let html = build_newtab_html(&[site("https://a.test/", "A")]);
        assert!(html.contains("title=\"Закрепить текущую страницу\""));
    }

    #[test]
    fn tile_initial_uses_host_first_letter() {
        assert_eq!(tile_initial("https://github.com/foo", "GitHub"), 'G');
        assert_eq!(tile_initial("https://www.rust-lang.org/", ""), 'R');
        // `www.` отбрасывается, берётся первая буква собственно хоста.
        assert_eq!(tile_initial("http://www.яндекс.рф/", ""), 'Я');
        // Хост без алфавитно-цифровых символов — fallback на заголовок.
        assert_eq!(tile_initial("", "Zed"), 'Z');
        // Совсем пусто — '?'.
        assert_eq!(tile_initial("", ""), '?');
    }

    #[test]
    fn badge_color_is_deterministic() {
        assert_eq!(badge_color('A'), badge_color('A'));
    }

    #[test]
    fn merge_tiles_pinned_first_then_top_sites_fill() {
        let pinned = vec![pinned_site("https://p.test/", "P")];
        let top = vec![site("https://t1.test/", "T1"), site("https://t2.test/", "T2")];
        let merged = merge_tiles(&pinned, &top);
        let urls: Vec<&str> = merged.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, vec!["https://p.test/", "https://t1.test/", "https://t2.test/"]);
    }

    #[test]
    fn merge_tiles_dedups_pinned_from_top_sites() {
        let pinned = vec![pinned_site("https://p.test/", "P")];
        let top = vec![site("https://p.test/", "P dup"), site("https://t.test/", "T")];
        let merged = merge_tiles(&pinned, &top);
        let urls: Vec<&str> = merged.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, vec!["https://p.test/", "https://t.test/"]);
    }

    #[test]
    fn merge_tiles_caps_at_seven_when_room_for_plus() {
        let top: Vec<TopSite> = (0..10)
            .map(|i| site(&format!("https://t{i}.test/"), "T"))
            .collect();
        let merged = merge_tiles(&[], &top);
        assert_eq!(merged.len(), MAX_SITE_TILES);
    }

    #[test]
    fn merge_tiles_allows_eight_when_all_pinned() {
        let pinned: Vec<TopSite> = (0..8)
            .map(|i| pinned_site(&format!("https://p{i}.test/"), "P"))
            .collect();
        let merged = merge_tiles(&pinned, &[site("https://t.test/", "T")]);
        assert_eq!(merged.len(), MAX_TILES);
        assert!(!merged.iter().any(|s| s.url == "https://t.test/"));
    }

    #[test]
    fn parse_action_plain_url_is_none() {
        assert_eq!(parse_action("https://example.com/"), None);
        assert_eq!(parse_action(NEWTAB_URL), None);
    }

    #[test]
    fn parse_action_pin_roundtrip() {
        let link = pin_link("https://a.test/?x=1&y=2", "Tom & Jerry");
        assert_eq!(
            parse_action(&link),
            Some(NewtabAction::Pin {
                url: "https://a.test/?x=1&y=2".to_string(),
                title: "Tom & Jerry".to_string(),
            })
        );
    }

    #[test]
    fn parse_action_unpin_roundtrip() {
        let link = unpin_link("https://a.test/");
        assert_eq!(
            parse_action(&link),
            Some(NewtabAction::Unpin { url: "https://a.test/".to_string() })
        );
    }

    #[test]
    fn parse_action_pin_current() {
        assert_eq!(parse_action(&pin_current_link()), Some(NewtabAction::PinCurrent));
    }

    #[test]
    fn parse_action_restore_closed() {
        assert_eq!(parse_action(&restore_closed_link()), Some(NewtabAction::RestoreClosed));
    }
}
