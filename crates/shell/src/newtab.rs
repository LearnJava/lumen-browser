//! `about:newtab` — статическая стартовая страница со «speed dial» из топ-5
//! наиболее посещаемых сайтов (по [`lumen_storage::History::most_visited`]).
//!
//! HTML генерируется в памяти ([`build_newtab_html`]) и грузится как
//! [`PageSource::Static`][crate::PageSource] — без сетевого запроса. Каждая
//! плитка — `<a href>` на URL сайта; «favicon» Phase 0 — цветной бейдж с первой
//! буквой хоста (без favicon-кэша).
//!
//! Phase 0 ограничения:
//! - максимум 5 плиток (топ-5 по `visit_count`);
//! - favicon-картинки не загружаются — вместо них буква-бейдж;
//! - страница статична: после открытия не реагирует на изменения истории.

/// Канонический URL стартовой страницы. Адресная строка и история показывают
/// именно эту строку.
pub const NEWTAB_URL: &str = "about:newtab";

/// Максимальное число плиток speed dial.
pub const MAX_TILES: usize = 5;

/// Одна плитка speed dial: целевой URL и отображаемый заголовок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopSite {
    /// Абсолютный URL сайта (`href` плитки).
    pub url: String,
    /// Человекочитаемый заголовок (берётся из истории; при пустом —
    /// подставляется хост URL вызывающей стороной).
    pub title: String,
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

/// Строит полный HTML страницы `about:newtab` со speed dial из `sites`.
///
/// Берётся не более [`MAX_TILES`] первых сайтов. При пустом списке плитки не
/// рисуются — выводится подсказка о том, что история пуста.
pub fn build_newtab_html(sites: &[TopSite]) -> String {
    let mut tiles = String::new();
    for site in sites.iter().take(MAX_TILES) {
        let initial = tile_initial(&site.url, &site.title);
        let color = badge_color(initial);
        let href = escape_html(&site.url);
        let title = escape_html(&site.title);
        tiles.push_str(&format!(
            "<a class=\"tile\" href=\"{href}\">\
               <div class=\"badge\" style=\"background:{color}\">{initial}</div>\
               <div class=\"name\">{title}</div>\
             </a>"
        ));
    }

    let body_inner = if tiles.is_empty() {
        "<p class=\"empty\">История пуста — открытые страницы появятся здесь.</p>".to_string()
    } else {
        format!("<div class=\"dial\">{tiles}</div>")
    };

    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\">\
<title>Новая вкладка</title><style>\
*{{box-sizing:border-box;}}\
body{{margin:0;background:#1b1b1f;color:#e8e8ea;\
font-family:'Inter',sans-serif;}}\
.wrap{{padding:120px 24px 24px 24px;}}\
h1{{text-align:center;font-size:30px;font-weight:600;\
margin:0 0 40px 0;color:#f4f4f6;}}\
.dial{{display:flex;flex-wrap:wrap;justify-content:center;}}\
.tile{{display:block;width:150px;height:128px;margin:10px;\
padding:18px 12px;background:#27272c;border-radius:14px;\
text-decoration:none;text-align:center;}}\
.badge{{width:52px;height:52px;margin:0 auto 14px auto;\
border-radius:14px;color:#ffffff;font-size:26px;font-weight:600;\
line-height:52px;text-align:center;}}\
.name{{color:#d6d6da;font-size:14px;line-height:18px;overflow:hidden;}}\
.empty{{text-align:center;color:#8a8a90;font-size:16px;}}\
</style></head><body><div class=\"wrap\"><h1>Lumen</h1>{body_inner}</div></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(url: &str, title: &str) -> TopSite {
        TopSite { url: url.to_string(), title: title.to_string() }
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
    fn html_limited_to_five_tiles() {
        let sites: Vec<TopSite> = (0..8)
            .map(|i| site(&format!("https://s{i}.test/"), &format!("Site {i}")))
            .collect();
        let html = build_newtab_html(&sites);
        let tile_count = html.matches("class=\"tile\"").count();
        assert_eq!(tile_count, MAX_TILES);
        // Шестой и далее сайты не попали.
        assert!(html.contains("https://s4.test/"));
        assert!(!html.contains("https://s5.test/"));
    }

    #[test]
    fn empty_sites_render_valid_page_without_tiles() {
        let html = build_newtab_html(&[]);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("</body></html>"));
        assert!(!html.contains("class=\"tile\""));
        assert!(html.contains("История пуста"));
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
}
