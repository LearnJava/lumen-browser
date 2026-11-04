//! Разбор текста cue WebVTT (спека WebVTT §6.4, «WebVTT cue text parsing rules»)
//! в дерево узлов [`CueNode`].
//!
//! Точка входа — [`parse_cue_text`]. Разбор повторяет машину состояний спеки
//! (data / escape / tag / start tag / start tag class / start tag annotation /
//! end tag / timestamp tag) и строит то же дерево WebVTT Node Objects, которое
//! §6.5 «WebVTT cue text DOM construction rules» отображает на DOM-элементы
//! `span`/`i`/`b`/`u`/`ruby`/`rt`. Срезы S3–S5 задачи VTT-RENDER строят из
//! этого дерева боксы cue в UA-тени `<video>`.

/// Тег элемента cue text — WebVTT Internal Node Object (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueTag {
    /// WebVTT Class Object — `<c.class …>`.
    C,
    /// WebVTT Italic Object — `<i>`.
    I,
    /// WebVTT Bold Object — `<b>`.
    B,
    /// WebVTT Underline Object — `<u>`.
    U,
    /// WebVTT Ruby Object — `<ruby>`.
    Ruby,
    /// WebVTT Ruby Text Object — `<rt>` (только внутри `<ruby>`).
    Rt,
    /// WebVTT Voice Object — `<v Имя>`.
    V,
    /// WebVTT Language Object — `<lang ru>`.
    Lang,
}

/// Узел дерева cue text (§6.4): текст, таймстамп или элемент.
#[derive(Debug, Clone, PartialEq)]
pub enum CueNode {
    /// WebVTT Text Object: фрагмент текста, сущности уже декодированы,
    /// переводы строк сохранены.
    Text(String),
    /// WebVTT Timestamp Object: время в секундах от начала медиа.
    Timestamp(f64),
    /// WebVTT Internal Node Object: элемент с тегом, классами и аннотацией.
    Element {
        /// Тег элемента.
        tag: CueTag,
        /// Применимые классы (пустые строки исключены по правилу «attach», §6.4).
        classes: Vec<String>,
        /// Аннотация `<v Имя>` / `<lang ru>`: пробелы по краям срезаны,
        /// пробельные последовательности сжаты в один пробел. `None`, если
        /// у тега не было разделителя перед аннотацией.
        annotation: Option<String>,
        /// Дочерние узлы.
        children: Vec<CueNode>,
    },
}

/// Разбирает текст cue (payload между строкей тайминга и концом блока,
/// разметка сохранена) в список узлов верхнего уровня.
///
/// Правила по §6.4: неизвестные теги пропускаются вместе с атрибутами;
/// `<rt>` создаётся только внутри `<ruby>`; закрывающий тег закрывает
/// ближайший совпадающий открытый (для совпадения он должен быть текущим
/// открытым элементом), прочие закрывающие теги игнорируются; невалидный
/// таймстамп игнорируется; из сущностей распознаются `&amp;` `&lt;` `&gt;`
/// `&lrm;` `&rlm;` `&nbsp;`, любая другая последовательность `&…;`
/// остаётся текстом; незакрытые теги закрываются в конце разбора.
pub fn parse_cue_text(text: &str) -> Vec<CueNode> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0usize;
    // Стек открытых элементов; нижний кадр — корневой список узлов.
    let mut stack: Vec<Frame> = vec![Frame::Root(Vec::new())];

    while let Some(token) = next_token(&chars, &mut pos) {
        match token {
            Token::Text(value) => {
                if !value.is_empty() {
                    push_child(&mut stack, CueNode::Text(value));
                }
            }
            Token::StartTag { name, classes, annotation } => {
                // Неизвестный тег пропускается целиком, вместе с классами и аннотацией.
                let Some(tag) = tag_from_name(&name) else { continue };
                // `<rt>` создаётся только когда текущий открытый элемент — `<ruby>`.
                if tag == CueTag::Rt
                    && !matches!(stack.last(), Some(Frame::El { tag: CueTag::Ruby, .. }))
                {
                    continue;
                }
                let classes: Vec<String> =
                    classes.into_iter().filter(|class| !class.is_empty()).collect();
                stack.push(Frame::El { tag, classes, annotation, children: Vec::new() });
            }
            Token::EndTag { name } => {
                let closes_current = match stack.last() {
                    Some(Frame::El { tag, .. }) => tag_matches_name(*tag, &name),
                    _ => false,
                };
                if closes_current {
                    close_top(&mut stack);
                } else if name == "ruby"
                    && matches!(stack.last(), Some(Frame::El { tag: CueTag::Rt, .. }))
                {
                    // Особый случай §6.4: `</ruby>` при открытом `<rt>` закрывает
                    // и rt, и сам ruby.
                    close_top(&mut stack);
                    if matches!(stack.last(), Some(Frame::El { tag: CueTag::Ruby, .. })) {
                        close_top(&mut stack);
                    }
                }
                // Прочие закрывающие теги игнорируются.
            }
            Token::TimestampTag { value } => {
                // Невалидный таймстамп (или хвост после него) игнорируется.
                if let Some(seconds) = collect_timestamp(&value) {
                    push_child(&mut stack, CueNode::Timestamp(seconds));
                }
            }
        }
    }

    // Незакрытые теги закрываются в конце разбора.
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    match stack.pop() {
        Some(Frame::Root(nodes)) => nodes,
        _ => Vec::new(),
    }
}

/// Кадр стека разбора: корневой список или открытый элемент.
enum Frame {
    /// Корневой список узлов (никогда не закрывается вручную).
    Root(Vec<CueNode>),
    /// Открытый элемент с накопленными детьми.
    El {
        tag: CueTag,
        classes: Vec<String>,
        annotation: Option<String>,
        children: Vec<CueNode>,
    },
}

impl Frame {
    /// Добавляет дочерний узел в кадр.
    fn push_child(&mut self, node: CueNode) {
        match self {
            Frame::Root(children) | Frame::El { children, .. } => children.push(node),
        }
    }
}

/// Токен машины состояний §6.4.
enum Token {
    /// Строка текста (сущности декодированы).
    Text(String),
    /// Открывающий тег: имя, классы, аннотация.
    StartTag {
        name: String,
        classes: Vec<String>,
        annotation: Option<String>,
    },
    /// Закрывающий тег с именем.
    EndTag {
        name: String,
    },
    /// Тег-таймстамп с сырым значением.
    TimestampTag {
        value: String,
    },
}

/// Добавляет узел в текущий верхний кадр стека.
fn push_child(stack: &mut [Frame], node: CueNode) {
    if let Some(frame) = stack.last_mut() {
        frame.push_child(node);
    }
}

/// Закрывает верхний открытый элемент, вклеивая его родителю.
fn close_top(stack: &mut Vec<Frame>) {
    if stack.len() < 2 {
        return;
    }
    if let Some(Frame::El { tag, classes, annotation, children }) = stack.pop() {
        push_child(stack, CueNode::Element { tag, classes, annotation, children });
    }
}

/// Имя тега → [`CueTag`]. Имена регистрозависимы, как в §6.4.
fn tag_from_name(name: &str) -> Option<CueTag> {
    match name {
        "c" => Some(CueTag::C),
        "i" => Some(CueTag::I),
        "b" => Some(CueTag::B),
        "u" => Some(CueTag::U),
        "ruby" => Some(CueTag::Ruby),
        "rt" => Some(CueTag::Rt),
        "v" => Some(CueTag::V),
        "lang" => Some(CueTag::Lang),
        _ => None,
    }
}

/// Совпадает ли имя закрывающего тега с тегом текущего открытого элемента.
fn tag_matches_name(tag: CueTag, name: &str) -> bool {
    matches!(
        (tag, name),
        (CueTag::C, "c")
            | (CueTag::I, "i")
            | (CueTag::B, "b")
            | (CueTag::U, "u")
            | (CueTag::Ruby, "ruby")
            | (CueTag::Rt, "rt")
            | (CueTag::V, "v")
            | (CueTag::Lang, "lang")
    )
}

/// Следующий токен из `chars` с позиции `pos` (data state §6.4).
/// `None` — вход исчерпан.
fn next_token(chars: &[char], pos: &mut usize) -> Option<Token> {
    let mut result = String::new();
    loop {
        let Some(&c) = chars.get(*pos) else {
            return (!result.is_empty()).then_some(Token::Text(result));
        };
        match c {
            '&' => {
                *pos += 1;
                match consume_entity(chars, pos) {
                    Some(decoded) => result.push(decoded),
                    None => result.push('&'),
                }
            }
            // '<' в начале токена переводит в tag state; иначе текстовый токен
            // отдаётся, а '<' останется началом следующего.
            '<' if result.is_empty() => {
                *pos += 1;
                return Some(tag_token(chars, pos));
            }
            '<' => return Some(Token::Text(result)),
            _ => {
                result.push(c);
                *pos += 1;
            }
        }
    }
}

/// Tag state §6.4: позиция — сразу после '<'.
fn tag_token(chars: &[char], pos: &mut usize) -> Token {
    let Some(&c) = chars.get(*pos) else {
        // EOF в tag state: start tag с пустым именем (построитель его игнорирует).
        return Token::StartTag { name: String::new(), classes: Vec::new(), annotation: None };
    };
    match c {
        // Пробелы (включая LF) переводят в start tag annotation state; по §6.4
        // LF попадает в буфер аннотации, но нормализация его срезает.
        '\t' | '\u{000C}' | ' ' | '\n' => {
            *pos += 1;
            start_tag_annotation(chars, pos, String::new(), Vec::new(), String::new())
        }
        '.' => {
            *pos += 1;
            start_tag_class(chars, pos, String::new(), Vec::new(), String::new())
        }
        '/' => {
            *pos += 1;
            end_tag(chars, pos)
        }
        c if c.is_ascii_digit() => {
            *pos += 1;
            timestamp_tag(chars, pos, c.to_string())
        }
        '>' => {
            // '>' в tag state: start tag с пустым именем.
            *pos += 1;
            Token::StartTag { name: String::new(), classes: Vec::new(), annotation: None }
        }
        c => {
            *pos += 1;
            start_tag(chars, pos, c.to_string())
        }
    }
}

/// Start tag state §6.4: `name` — уже набранное имя тега.
fn start_tag(chars: &[char], pos: &mut usize, mut name: String) -> Token {
    loop {
        let Some(&c) = chars.get(*pos) else {
            return Token::StartTag { name, classes: Vec::new(), annotation: None };
        };
        match c {
            '\t' | '\u{000C}' | ' ' | '\n' => {
                *pos += 1;
                return start_tag_annotation(chars, pos, name, Vec::new(), String::new());
            }
            '.' => {
                *pos += 1;
                return start_tag_class(chars, pos, name, Vec::new(), String::new());
            }
            '>' => {
                *pos += 1;
                return Token::StartTag { name, classes: Vec::new(), annotation: None };
            }
            c => {
                name.push(c);
                *pos += 1;
            }
        }
    }
}

/// Start tag class state §6.4: `class_buf` — набираемый класс.
fn start_tag_class(
    chars: &[char],
    pos: &mut usize,
    name: String,
    mut classes: Vec<String>,
    mut class_buf: String,
) -> Token {
    loop {
        let Some(&c) = chars.get(*pos) else {
            classes.push(class_buf);
            return Token::StartTag { name, classes, annotation: None };
        };
        match c {
            '\t' | '\u{000C}' | ' ' | '\n' => {
                classes.push(std::mem::take(&mut class_buf));
                *pos += 1;
                return start_tag_annotation(chars, pos, name, classes, String::new());
            }
            '.' => {
                classes.push(std::mem::take(&mut class_buf));
                *pos += 1;
            }
            '>' => {
                *pos += 1;
                classes.push(class_buf);
                return Token::StartTag { name, classes, annotation: None };
            }
            c => {
                class_buf.push(c);
                *pos += 1;
            }
        }
    }
}

/// Start tag annotation state §6.4: `buffer` — набираемая аннотация.
fn start_tag_annotation(
    chars: &[char],
    pos: &mut usize,
    name: String,
    classes: Vec<String>,
    mut buffer: String,
) -> Token {
    loop {
        let Some(&c) = chars.get(*pos) else {
            let annotation = Some(normalize_annotation(&buffer));
            return Token::StartTag { name, classes, annotation };
        };
        match c {
            '&' => {
                *pos += 1;
                match consume_entity(chars, pos) {
                    Some(decoded) => buffer.push(decoded),
                    None => buffer.push('&'),
                }
            }
            '>' => {
                *pos += 1;
                let annotation = Some(normalize_annotation(&buffer));
                return Token::StartTag { name, classes, annotation };
            }
            c => {
                buffer.push(c);
                *pos += 1;
            }
        }
    }
}

/// End tag state §6.4.
fn end_tag(chars: &[char], pos: &mut usize) -> Token {
    let mut name = String::new();
    loop {
        let Some(&c) = chars.get(*pos) else {
            return Token::EndTag { name };
        };
        match c {
            '>' => {
                *pos += 1;
                return Token::EndTag { name };
            }
            c => {
                name.push(c);
                *pos += 1;
            }
        }
    }
}

/// Timestamp tag state §6.4: `value` — уже набранное начало значения.
fn timestamp_tag(chars: &[char], pos: &mut usize, mut value: String) -> Token {
    loop {
        let Some(&c) = chars.get(*pos) else {
            return Token::TimestampTag { value };
        };
        match c {
            '>' => {
                *pos += 1;
                return Token::TimestampTag { value };
            }
            c => {
                value.push(c);
                *pos += 1;
            }
        }
    }
}

/// Пробует потребить именованную сущность после уже съеденного `&`
/// (упрощение «HTML character reference» из §6.4 до шести сущностей спеки:
/// `&amp;` `&lt;` `&gt;` `&lrm;` `&rlm;` `&nbsp;`). При совпадении потребляет
/// имя и `;`, возвращает символ-замену; иначе не потребляет ничего.
fn consume_entity(chars: &[char], pos: &mut usize) -> Option<char> {
    const ENTITIES: [(&str, char); 6] = [
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("lrm", '\u{200E}'),
        ("rlm", '\u{200F}'),
        ("nbsp", '\u{00A0}'),
    ];
    let tail = &chars[*pos..];
    for (name, replacement) in ENTITIES {
        let matched = tail.len() > name.len()
            && tail[..name.len()].iter().copied().eq(name.chars())
            && tail[name.len()] == ';';
        if matched {
            *pos += name.len() + 1;
            return Some(replacement);
        }
    }
    None
}

/// ASCII-пробел по HTML: tab, LF, FF, CR, space.
fn is_ascii_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000C}' | '\r' | ' ')
}

/// Нормализация аннотации §6.4: срезать пробелы по краям, сжать
/// последовательности ASCII-пробелов в один U+0020.
fn normalize_annotation(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.trim_matches(is_ascii_whitespace).chars() {
        if is_ascii_whitespace(c) {
            pending_space = true;
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    out
}

/// «Collect a WebVTT timestamp»: `mm:ss.ttt` или `hh:mm:ss.ttt` (поле часов —
/// одна и более цифр; минутное поле из двух цифр со значением > 59 считается
/// часами). Вся строка должна быть таймстампом без хвоста. Возвращает секунды.
fn collect_timestamp(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;

    // Поле 1; не две цифры или значение > 59 — старшая единица это часы.
    let (value1, len1) = collect_digits(&chars, &mut i)?;
    let mut hours = len1 != 2 || value1 > 59;
    if chars.get(i) != Some(&':') {
        return None;
    }
    i += 1;
    let (value2, len2) = collect_digits(&chars, &mut i)?;
    if len2 != 2 {
        return None;
    }
    // Три поля — если старшая единица часы или дальше снова ':'.
    hours = hours || chars.get(i) == Some(&':');

    let (h, m, s) = if hours {
        if chars.get(i) != Some(&':') {
            return None;
        }
        i += 1;
        let (value3, len3) = collect_digits(&chars, &mut i)?;
        if len3 != 2 {
            return None;
        }
        (value1, value2, value3)
    } else {
        (0, value1, value2)
    };
    if chars.get(i) != Some(&'.') {
        return None;
    }
    i += 1;
    let (ms, len_ms) = collect_digits(&chars, &mut i)?;
    if len_ms != 3 {
        return None;
    }
    if m > 59 || s > 59 {
        return None;
    }
    // Позиция обязана указывать на конец значения (§6.4, шаг timestamp tag).
    if i != chars.len() {
        return None;
    }
    Some(h as f64 * 3600.0 + m as f64 * 60.0 + s as f64 + ms as f64 / 1000.0)
}

/// Собирает последовательность ASCII-цифр с `i`; возвращает значение и длину.
/// `None` — ни одной цифры (или переполнение u64).
fn collect_digits(chars: &[char], i: &mut usize) -> Option<(u64, usize)> {
    let start = *i;
    let mut value: u64 = 0;
    while let Some(&c) = chars.get(*i) {
        let Some(d) = c.to_digit(10) else { break };
        value = value.checked_mul(10)?.checked_add(u64::from(d))?;
        *i += 1;
    }
    let len = *i - start;
    (len >= 1).then_some((value, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> CueNode {
        CueNode::Text(text.to_string())
    }

    fn ts(seconds: f64) -> CueNode {
        CueNode::Timestamp(seconds)
    }

    fn el(
        tag: CueTag,
        classes: &[&str],
        annotation: Option<&str>,
        children: Vec<CueNode>,
    ) -> CueNode {
        CueNode::Element {
            tag,
            classes: classes.iter().map(|s| (*s).to_string()).collect(),
            annotation: annotation.map(str::to_string),
            children,
        }
    }

    /// Текст cue из фикстуры WPT: файл читается из checkout-а, в котором
    /// компилировался крейт (`CARGO_MANIFEST_DIR`), — не из cwd теста.
    fn first_fixture_cue_text(file: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/wpt/webvtt/rendering/cues-with-video/processing-model/support")
            .join(file);
        let cues = crate::parse_vtt(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(cues.len(), 1, "в {file} ожидается один cue");
        cues[0].text.clone()
    }

    #[test]
    fn empty_input_is_empty_vec() {
        assert_eq!(parse_cue_text(""), Vec::new());
    }

    #[test]
    fn plain_text_single_node() {
        assert_eq!(parse_cue_text("Hello"), vec![t("Hello")]);
    }

    #[test]
    fn nested_bold_italic() {
        let nodes = parse_cue_text("<b><i>x</i></b>");
        assert_eq!(nodes, vec![el(CueTag::B, &[], None, vec![el(CueTag::I, &[], None, vec![t("x")])])]);
    }

    #[test]
    fn class_tag_with_multiple_classes() {
        let nodes = parse_cue_text("y <c.red.big>z</c> w");
        assert_eq!(
            nodes,
            vec![
                t("y "),
                el(CueTag::C, &["red", "big"], None, vec![t("z")]),
                t(" w"),
            ]
        );
    }

    #[test]
    fn class_tag_empty_class_excluded() {
        let nodes = parse_cue_text("<c..z>x</c>");
        assert_eq!(nodes, vec![el(CueTag::C, &["z"], None, vec![t("x")])]);
    }

    #[test]
    fn voice_tag_annotation() {
        let nodes = parse_cue_text("<v Fred Smith>hi</v>");
        assert_eq!(nodes, vec![el(CueTag::V, &[], Some("Fred Smith"), vec![t("hi")])]);
    }

    #[test]
    fn voice_tag_without_annotation() {
        let nodes = parse_cue_text("<v>hi</v>");
        assert_eq!(nodes, vec![el(CueTag::V, &[], None, vec![t("hi")])]);
    }

    #[test]
    fn lang_tag_annotation() {
        let nodes = parse_cue_text("Deutsch<lang en-US>English</lang>");
        assert_eq!(
            nodes,
            vec![t("Deutsch"), el(CueTag::Lang, &[], Some("en-US"), vec![t("English")])]
        );
    }

    #[test]
    fn ruby_with_rt() {
        let nodes = parse_cue_text("<ruby>x<rt>y</rt></ruby>");
        assert_eq!(
            nodes,
            vec![el(CueTag::Ruby, &[], None, vec![t("x"), el(CueTag::Rt, &[], None, vec![t("y")])])]
        );
    }

    #[test]
    fn rt_outside_ruby_skipped() {
        // Открывающий и закрывающий <rt> вне ruby игнорируются, текст остаётся.
        let nodes = parse_cue_text("a<rt>y</rt>b");
        assert_eq!(nodes, vec![t("a"), t("y"), t("b")]);
    }

    #[test]
    fn rt_inside_non_ruby_child_of_ruby_skipped() {
        // <rt> допустим только когда текущий открытый элемент — сам ruby (§6.4).
        let nodes = parse_cue_text("<ruby><b><rt>y</rt></b></ruby>");
        assert_eq!(
            nodes,
            vec![el(CueTag::Ruby, &[], None, vec![el(CueTag::B, &[], None, vec![t("y")])])]
        );
    }

    #[test]
    fn unclosed_tags_closed_at_end() {
        let nodes = parse_cue_text("a<b>b<i>c");
        assert_eq!(
            nodes,
            vec![
                t("a"),
                el(CueTag::B, &[], None, vec![t("b"), el(CueTag::I, &[], None, vec![t("c")])]),
            ]
        );
    }

    #[test]
    fn stray_end_tag_ignored() {
        // Лишний </i> без открытого элемента игнорируется.
        let nodes = parse_cue_text("x</i>y");
        assert_eq!(nodes, vec![t("x"), t("y")]);
    }

    #[test]
    fn end_tag_not_matching_current_ignored() {
        // </b> при текущем открытом <i> не закрывает его (§6.4 сверяет только
        // current), поэтому оба </b> игнорируются и «y» уходит внутрь <i>.
        let nodes = parse_cue_text("<b><i>x</b></b>y");
        assert_eq!(
            nodes,
            vec![el(
                CueTag::B,
                &[],
                None,
                vec![el(CueTag::I, &[], None, vec![t("x"), t("y")])]
            )]
        );
    }

    #[test]
    fn end_ruby_closes_open_rt() {
        // Особый случай §6.4: </ruby> при открытом <rt> закрывает и rt, и ruby.
        let nodes = parse_cue_text("<ruby>x<rt>y</ruby>");
        assert_eq!(
            nodes,
            vec![el(CueTag::Ruby, &[], None, vec![t("x"), el(CueTag::Rt, &[], None, vec![t("y")])])]
        );
    }

    #[test]
    fn timestamp_between_words() {
        let nodes = parse_cue_text("start <00:00.001> middle");
        assert_eq!(nodes, vec![t("start "), ts(0.001), t(" middle")]);
    }

    #[test]
    fn timestamp_full_hours_form() {
        let nodes = parse_cue_text("<01:02:03.500>x");
        assert_eq!(nodes, vec![ts(3723.5), t("x")]);
    }

    #[test]
    fn invalid_timestamp_ignored() {
        // Минутное поле из одной цифры трактуется как часы и требует второй ':'.
        assert_eq!(parse_cue_text("a <0:00.000> b"), vec![t("a "), t(" b")]);
        assert_eq!(parse_cue_text("a <00:0a.000> b"), vec![t("a "), t(" b")]);
        // Хвост после таймстампа делает тег невалидным.
        assert_eq!(parse_cue_text("a <00:00.000x> b"), vec![t("a "), t(" b")]);
    }

    #[test]
    fn entity_amp() {
        assert_eq!(parse_cue_text("a&amp;b"), vec![t("a&b")]);
    }

    #[test]
    fn entity_lt() {
        assert_eq!(parse_cue_text("a&lt;b"), vec![t("a<b")]);
    }

    #[test]
    fn entity_gt() {
        assert_eq!(parse_cue_text("a&gt;b"), vec![t("a>b")]);
    }

    #[test]
    fn entity_lrm() {
        assert_eq!(parse_cue_text("a&lrm;b"), vec![t("a\u{200E}b")]);
    }

    #[test]
    fn entity_rlm() {
        assert_eq!(parse_cue_text("a&rlm;b"), vec![t("a\u{200F}b")]);
    }

    #[test]
    fn entity_nbsp() {
        assert_eq!(parse_cue_text("a&nbsp;b"), vec![t("a\u{00A0}b")]);
    }

    #[test]
    fn unknown_entity_kept_as_text() {
        assert_eq!(parse_cue_text("x &foo; y"), vec![t("x &foo; y")]);
    }

    #[test]
    fn bare_ampersand_kept() {
        assert_eq!(parse_cue_text("Tom & Jerry"), vec![t("Tom & Jerry")]);
        assert_eq!(parse_cue_text("a&"), vec![t("a&")]);
    }

    #[test]
    fn unknown_tag_with_attributes_skipped() {
        // Неизвестный тег пропускается вместе с атрибутами; регистр важен.
        let nodes = parse_cue_text("a<x foo=1 bar>b</x>c <B>bold</B>");
        assert_eq!(nodes, vec![t("a"), t("b"), t("c "), t("bold")]);
    }

    #[test]
    fn newline_preserved_in_text() {
        let nodes = parse_cue_text("line1\n<b>line2");
        assert_eq!(nodes, vec![t("line1\n"), el(CueTag::B, &[], None, vec![t("line2")])]);
    }

    #[test]
    fn annotation_whitespace_collapsed() {
        let nodes = parse_cue_text("<v   Fred   Smith  >hi</v>");
        assert_eq!(nodes, vec![el(CueTag::V, &[], Some("Fred Smith"), vec![t("hi")])]);
    }

    #[test]
    fn annotation_entity_decoded() {
        let nodes = parse_cue_text("<v Fred &amp; Barney>hi</v>");
        assert_eq!(nodes, vec![el(CueTag::V, &[], Some("Fred & Barney"), vec![t("hi")])]);
    }

    #[test]
    fn fixture_test_voice() {
        let text = first_fixture_cue_text("test_voice.vtt");
        assert_eq!(text, "This is a <v foo>test subtitle</v>");
        assert_eq!(
            parse_cue_text(&text),
            vec![
                t("This is a "),
                el(CueTag::V, &[], Some("foo"), vec![t("test subtitle")]),
            ]
        );
    }

    #[test]
    fn fixture_test_two_voices() {
        let text = first_fixture_cue_text("test_two_voices.vtt");
        assert_eq!(
            parse_cue_text(&text),
            vec![
                t("This is a "),
                el(CueTag::V, &[], Some("foo"), vec![t("test subtitle")]),
                t(", "),
                el(CueTag::V, &[], Some("bar"), vec![t("test subtitle")]),
            ]
        );
    }

    #[test]
    fn fixture_test_lang_object() {
        let text = first_fixture_cue_text("test_lang_object.vtt");
        assert_eq!(
            parse_cue_text(&text),
            vec![t("Deutsch"), el(CueTag::Lang, &[], Some("en"), vec![t("English")])]
        );
    }

    #[test]
    fn fixture_bold_with_timestamp() {
        let text = first_fixture_cue_text("bold_with_timestamp.vtt");
        assert_eq!(
            parse_cue_text(&text),
            vec![
                ts(0.0),
                el(CueTag::B, &[], None, vec![t("This is a test subtitle")]),
                ts(0.001),
            ]
        );
    }

    #[test]
    fn fixture_decode_escaped_entities() {
        let text = first_fixture_cue_text("decode_escaped_entities.vtt");
        assert_eq!(
            parse_cue_text(&text),
            vec![t("Here are the escaped entities: & < > \u{200E} \u{200F} \u{00A0}")]
        );
    }
}
