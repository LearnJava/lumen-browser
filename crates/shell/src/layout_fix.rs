//! Исправление неверной раскладки (UX-LAYOUT-FIX): ввод, набранный целиком
//! в одной раскладке (`ghbdtn`, `млюсщь`), переводится в другую по физическим
//! клавишам ЙЦУКЕН ↔ QWERTY.
//!
//! Чистая модель без доступа к хранилищам: [`convert`] — сам перевод,
//! [`suggest`] — решение «показывать ли исправление» (перевод должен дать
//! что-то известное: хост из списка, слово из истории/закладок — это проверяет
//! вызывающий через замыкание `known`). Адресная строка показывает результат
//! первой подсказкой, не подменяя набранное; Ctrl+F применяет перевод, только
//! если по исходному запросу совпадений нет.
//!
//! Пароли и поля страниц сюда не попадают: вызывают только из адресной строки
//! и строки поиска по странице.

/// Латиница → кириллица по физическим клавишам (буквенный ряд и знаки,
/// которые на русской раскладке стоят на месте `, . ; ' [ ] \``).
fn latin_to_cyrillic(c: char) -> Option<char> {
    Some(match c {
        'q' => 'й', 'w' => 'ц', 'e' => 'у', 'r' => 'к', 't' => 'е', 'y' => 'н', 'u' => 'г',
        'i' => 'ш', 'o' => 'щ', 'p' => 'з', '[' => 'х', ']' => 'ъ',
        'a' => 'ф', 's' => 'ы', 'd' => 'в', 'f' => 'а', 'g' => 'п', 'h' => 'р', 'j' => 'о',
        'k' => 'л', 'l' => 'д', ';' => 'ж', '\'' => 'э',
        'z' => 'я', 'x' => 'ч', 'c' => 'с', 'v' => 'м', 'b' => 'и', 'n' => 'т', 'm' => 'ь',
        ',' => 'б', '.' => 'ю', '`' => 'ё', '/' => '.', '?' => ',',
        'Q' => 'Й', 'W' => 'Ц', 'E' => 'У', 'R' => 'К', 'T' => 'Е', 'Y' => 'Н', 'U' => 'Г',
        'I' => 'Ш', 'O' => 'Щ', 'P' => 'З', '{' => 'Х', '}' => 'Ъ',
        'A' => 'Ф', 'S' => 'Ы', 'D' => 'В', 'F' => 'А', 'G' => 'П', 'H' => 'Р', 'J' => 'О',
        'K' => 'Л', 'L' => 'Д', ':' => 'Ж', '"' => 'Э',
        'Z' => 'Я', 'X' => 'Ч', 'C' => 'С', 'V' => 'М', 'B' => 'И', 'N' => 'Т', 'M' => 'Ь',
        '<' => 'Б', '>' => 'Ю', '~' => 'Ё',
        _ => return None,
    })
}

/// Обратная таблица: кириллица → латиница той же клавиши.
fn cyrillic_to_latin(c: char) -> Option<char> {
    Some(match c {
        'й' => 'q', 'ц' => 'w', 'у' => 'e', 'к' => 'r', 'е' => 't', 'н' => 'y', 'г' => 'u',
        'ш' => 'i', 'щ' => 'o', 'з' => 'p', 'х' => '[', 'ъ' => ']',
        'ф' => 'a', 'ы' => 's', 'в' => 'd', 'а' => 'f', 'п' => 'g', 'р' => 'h', 'о' => 'j',
        'л' => 'k', 'д' => 'l', 'ж' => ';', 'э' => '\'',
        'я' => 'z', 'ч' => 'x', 'с' => 'c', 'м' => 'v', 'и' => 'b', 'т' => 'n', 'ь' => 'm',
        'б' => ',', 'ю' => '.', 'ё' => '`', '.' => '/', ',' => '?',
        'Й' => 'Q', 'Ц' => 'W', 'У' => 'E', 'К' => 'R', 'Е' => 'T', 'Н' => 'Y', 'Г' => 'U',
        'Ш' => 'I', 'Щ' => 'O', 'З' => 'P', 'Х' => '{', 'Ъ' => '}',
        'Ф' => 'A', 'Ы' => 'S', 'В' => 'D', 'А' => 'F', 'П' => 'G', 'Р' => 'H', 'О' => 'J',
        'Л' => 'K', 'Д' => 'L', 'Ж' => ':', 'Э' => '"',
        'Я' => 'Z', 'Ч' => 'X', 'С' => 'C', 'М' => 'V', 'И' => 'B', 'Т' => 'N', 'Ь' => 'M',
        'Б' => '<', 'Ю' => '>', 'Ё' => '~',
        _ => return None,
    })
}

fn is_cyrillic_letter(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}') && c.is_alphabetic()
}

/// Перевод `input` в соседнюю раскладку. `None`, если в тексте нет букв, буквы
/// двух алфавитов смешаны (раскладку не меняли), в нём есть схема `://` или
/// `@`/`!` в начале (служебные команды омнибокса), либо перевод ничего не
/// меняет. Цифры, пробелы и неизвестные знаки остаются как есть.
pub(crate) fn convert(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.contains("://") || trimmed.starts_with(['@', '!']) {
        return None;
    }
    let latin = trimmed.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let cyr = trimmed.chars().filter(|&c| is_cyrillic_letter(c)).count();
    if (latin == 0) == (cyr == 0) || trimmed.chars().any(|c| c.is_alphabetic() && !c.is_ascii() && !is_cyrillic_letter(c)) {
        return None;
    }
    let to_cyr = latin > 0;
    let out: String = trimmed
        .chars()
        .map(|c| {
            let m = if to_cyr { latin_to_cyrillic(c) } else { cyrillic_to_latin(c) };
            m.unwrap_or(c)
        })
        .collect();
    (out != trimmed).then_some(out)
}

/// Хосты, которые считаются известными без истории. Только то, что русскоязычный
/// пользователь действительно набирает в адресной строке.
const KNOWN_HOSTS: &[&str] = &[
    "vk.com", "ok.ru", "yandex.ru", "ya.ru", "mail.ru", "rambler.ru", "dzen.ru", "rutube.ru",
    "gosuslugi.ru", "sberbank.ru", "tinkoff.ru", "avito.ru", "ozon.ru", "wildberries.ru",
    "hh.ru", "kinopoisk.ru", "lenta.ru", "rbc.ru", "habr.com", "2gis.ru", "drom.ru", "cian.ru",
    "mos.ru", "nalog.gov.ru", "telegram.org", "web.telegram.org", "t.me",
    "google.com", "google.ru", "youtube.com", "gmail.com", "github.com", "gitlab.com",
    "stackoverflow.com", "reddit.com", "twitter.com", "x.com", "facebook.com", "instagram.com",
    "linkedin.com", "twitch.tv", "netflix.com", "amazon.com", "ebay.com", "aliexpress.com",
    "apple.com", "microsoft.com", "live.com", "outlook.com", "bing.com", "duckduckgo.com",
    "wikipedia.org", "mozilla.org", "rust-lang.org", "crates.io", "docs.rs", "npmjs.com",
    "openai.com", "anthropic.com", "claude.ai", "spotify.com", "discord.com", "whatsapp.com",
    "cloudflare.com", "yahoo.com", "baidu.com", "booking.com", "paypal.com", "dropbox.com",
];

/// Частые слова запросов: ровно столько, чтобы исправление работало на чистом
/// профиле; остальное решает история и закладки.
const COMMON_WORDS: &[&str] = &[
    "привет", "пока", "погода", "новости", "курс", "доллар", "евро", "карты", "карта", "почта",
    "музыка", "фильмы", "фильм", "сериал", "игры", "работа", "вакансии", "перевод", "переводчик",
    "словарь", "рецепт", "рецепты", "расписание", "билеты", "такси", "магазин", "купить",
    "скачать", "бесплатно", "онлайн", "как", "что", "где", "когда", "почему", "сколько",
    "время", "адрес", "телефон", "сайт", "поиск", "найти", "википедия", "гороскоп", "банк",
    "hello", "weather", "translate", "mail", "news", "music", "video",
];

/// Хост из `candidate` (до первого `/`, `?`, `#`), без `www.`, в нижнем регистре.
fn host_of(candidate: &str) -> String {
    let host = candidate.split(['/', '?', '#']).next().unwrap_or("").to_lowercase();
    host.strip_prefix("www.").map(str::to_owned).unwrap_or(host)
}

/// `candidate` — хост (возможно с путём) из списка известных.
pub(crate) fn is_known_host(candidate: &str) -> bool {
    let host = host_of(candidate.trim());
    !host.is_empty() && KNOWN_HOSTS.contains(&host.as_str())
}

fn is_common_word(token: &str) -> bool {
    let t = token.to_lowercase();
    COMMON_WORDS.contains(&t.as_str())
}

/// Результат [`suggest`]: что показать первой подсказкой.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayoutFix {
    /// Переведённый текст — он же значение подсказки при выборе.
    pub corrected: String,
}

/// Самая короткая строка (в символах без пробелов), которую вообще пробуем
/// исправлять: на двух буквах перевод почти всегда случайно «известен».
const MIN_FIX_LEN: usize = 3;

/// Решает, предлагать ли исправление раскладки для `input`.
///
/// `known(text)` — проверка по хранилищам вызывающего (история, закладки,
/// прошлые запросы): «это слово/адрес пользователь уже встречал». Исправление
/// предлагается, только если сам `input` неизвестен, а его перевод известен —
/// хост из списка, слово из `known`/списка частых слов (для нескольких слов —
/// каждое).
pub(crate) fn suggest(input: &str, known: impl Fn(&str) -> bool) -> Option<LayoutFix> {
    let input = input.trim();
    if input.chars().filter(|c| !c.is_whitespace()).count() < MIN_FIX_LEN {
        return None;
    }
    let corrected = convert(input)?;
    if is_known_host(input) || known(input) {
        return None;
    }
    let token_known = |t: &str| is_known_host(t) || is_common_word(t) || known(t);
    let accepted = is_known_host(&corrected)
        || known(&corrected)
        || corrected.split_whitespace().all(token_known);
    accepted.then_some(LayoutFix { corrected })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: &str) -> bool {
        false
    }

    #[test]
    fn converts_letters_both_ways() {
        assert_eq!(convert("ghbdtn").as_deref(), Some("привет"));
        assert_eq!(convert("привет").as_deref(), Some("ghbdtn"));
        assert_eq!(convert("Ghbdtn Vbh").as_deref(), Some("Привет Мир"));
    }

    #[test]
    fn converts_punctuation_keys() {
        assert_eq!(convert("млюсщь").as_deref(), Some("vk.com"));
        assert_eq!(convert("vk.com").as_deref(), Some("млюсщь"));
        assert_eq!(convert("a,.;'[]`").as_deref(), Some("фбюжэхъё"));
        assert_eq!(convert("фбюжэхъё").as_deref(), Some("a,.;'[]`"));
        assert_eq!(convert("A<>:\"{}~").as_deref(), Some("ФБЮЖЭХЪЁ"));
    }

    #[test]
    fn mixed_scripts_and_services_are_left_alone() {
        assert_eq!(convert("ghbдtn"), None);
        assert_eq!(convert("https://vk.com"), None);
        assert_eq!(convert("@history ghbdtn"), None);
        assert_eq!(convert("!g ghbdtn"), None);
        assert_eq!(convert("12345"), None);
        assert_eq!(convert(""), None);
        assert_eq!(convert("naïve"), None);
    }

    #[test]
    fn digits_and_spaces_survive() {
        assert_eq!(convert("ghbdtn 2024").as_deref(), Some("привет 2024"));
    }

    #[test]
    fn suggests_word_from_builtin_list() {
        let fix = suggest("ghbdtn", nothing).expect("fix");
        assert_eq!(fix.corrected, "привет");
    }

    #[test]
    fn suggests_known_host() {
        let fix = suggest("млюсщь", nothing).expect("fix");
        assert_eq!(fix.corrected, "vk.com");
        let fix = suggest("млюсщь.ауув", nothing);
        assert_eq!(fix.map(|f| f.corrected).as_deref(), Some("vk.com/feed"));
    }

    #[test]
    fn suggests_word_from_history() {
        let fix = suggest("ntcn", |t| t == "тест").expect("fix");
        assert_eq!(fix.corrected, "тест");
        // Неизвестный перевод — ничего.
        assert_eq!(suggest("ntcn", nothing), None);
    }

    #[test]
    fn every_word_of_a_phrase_must_be_known() {
        assert_eq!(suggest("gjujlf rehc", nothing).map(|f| f.corrected), Some("погода курс".into()));
        assert_eq!(suggest("gjujlf ntcnjdsq", nothing), None);
    }

    #[test]
    fn known_original_is_not_rewritten() {
        assert_eq!(suggest("github.com", nothing), None);
        assert_eq!(suggest("ghbdtn", |t| t == "ghbdtn"), None);
    }

    #[test]
    fn too_short_input_is_ignored() {
        assert_eq!(suggest("ьр", nothing), None);
    }

    #[test]
    fn tables_are_inverse() {
        for c in "qwertyuiop[]asdfghjkl;'zxcvbnm,.`QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>~/?".chars() {
            let cyr = latin_to_cyrillic(c).unwrap_or_else(|| panic!("no mapping for {c}"));
            assert_eq!(cyrillic_to_latin(cyr), Some(c), "{c} -> {cyr}");
        }
    }
}
