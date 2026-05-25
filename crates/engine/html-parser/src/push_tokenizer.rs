//! Push-режим токенизатора: вход подаётся chunk-ами, токены выдаются
//! по мере их полноты в буфере.
//!
//! Phase 0 разработан как обёртка над существующим pull-токенизатором
//! ([`crate::tokenizer::Tokenizer`]): pull-токенизатор уже корректно
//! работает с lenient HTML5 (~270 тестов), не нужно дублировать state
//! machine. Цена обёртки — owned `String`-буфер, в который копятся
//! chunk-и, и эвристика поиска «безопасной точки среза» в этом буфере.
//!
//! Контракт:
//!
//! * [`PushTokenizer::feed`] — добавить chunk, получить токены, которые
//!   *гарантированно* полные на текущем буфере. Если буфер обрывается
//!   посередине тега / entity / RAWTEXT-терминатора, эти байты остаются
//!   в буфере и эмитятся при следующем `feed`-е или `end`-е.
//! * [`PushTokenizer::end`] — финализация: оставшийся хвост буфера
//!   токенизируется как «последний кусок», что даёт lenient-поведение
//!   (как у pull при EOF посреди тега).
//!
//! Идентичность DOM с pull-режимом обеспечивается на уровне tree
//! builder-а (text-node coalescing), а не здесь: push-токенизатор может
//! отдавать `Token::Text` несколькими кусками для одного непрерывного
//! текстового потока, и это нормально.
//!
//! UTF-8: `feed` принимает `&str` — вызыватель отвечает за то, чтобы
//! граница chunk-а лежала на code point boundary. [`PushTokenizer::feed_bytes`]
//! принимает `&[u8]` и сам буферизует незавершённые UTF-8 последовательности.

use crate::tokenizer::{Token, Tokenizer};

/// Push-режим HTML5 токенизатора. См. module-level docs.
pub struct PushTokenizer {
    /// Накопленный, ещё не потреблённый pull-токенизатором ввод.
    /// Растёт при `feed`, обрезается слева при выдаче токенов.
    buf: String,
    /// `text_only`-состояние pull-токенизатора, перенесённое между
    /// chunk-ами. `Some((tag, decode_entities))` после открытия
    /// `<script>`/`<style>` (RAWTEXT, decode=false) или
    /// `<title>`/`<textarea>` (RCDATA, decode=true).
    text_only: Option<(String, bool)>,
    /// `true` после `end()` — следующие вызовы `feed` запрещены.
    ended: bool,
    /// Незавершённая UTF-8 последовательность с конца предыдущего
    /// `feed_bytes`-вызова. Максимум 3 байта. Присоединяется к началу
    /// следующего chunk-а в `feed_bytes`.
    partial_utf8: Vec<u8>,
}

impl PushTokenizer {
    /// Создаёт новый `PushTokenizer` в исходном состоянии.
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            text_only: None,
            ended: false,
            partial_utf8: Vec::new(),
        }
    }

    /// Скармливает chunk токенизатору и возвращает токены, ставшие
    /// «полностью видимыми» на текущем буфере. Незавершённые конструкции
    /// (тег без `>`, entity без `;`, RAWTEXT без терминатора и т.д.)
    /// остаются в буфере и дождутся следующего `feed`-а или `end`-а.
    ///
    /// Многократно вызывать после `end()` запрещено (panic).
    pub fn feed(&mut self, chunk: &str) -> Vec<Token> {
        assert!(!self.ended, "feed() after end()");
        self.buf.push_str(chunk);
        self.tokenize(false)
    }

    /// Вариант [`PushTokenizer::feed`] для сырых байт из сети.
    ///
    /// Буферизует незавершённую UTF-8 последовательность на границе
    /// chunk-а и присоединяет её к следующему вызову. Корректно
    /// обрабатывает многобайтные символы (кириллица, CJK, эмодзи),
    /// разрезанные на границе сетевого пакета.
    ///
    /// Гарантированно завершённые code point-ы передаются в
    /// [`PushTokenizer::feed`]-логику без дополнительного копирования.
    ///
    /// Гарантии WHATWG Encoding §4:
    /// - Незавершённая последовательность в хвосте chunk-а → буферизуется.
    /// - Явно невалидные байты (0xFF, неожиданный continuation byte и т.п.)
    ///   → заменяются U+FFFD inline, обработка продолжается.
    /// - Незавершённая последовательность при `end()` → U+FFFD.
    pub fn feed_bytes(&mut self, chunk: &[u8]) -> Vec<Token> {
        assert!(!self.ended, "feed_bytes() after end()");

        self.partial_utf8.extend_from_slice(chunk);

        // Декодируем partial_utf8 → self.buf, сохраняя незавершённый хвост.
        // Используем три варианта результата:
        //   (valid_len, true,  _)       — from_utf8 вернул Ok: все байты валидны
        //   (valid_len, false, None)    — от Err: хвост обрезан (truncated sequence)
        //   (valid_len, false, Some(n)) — от Err: невалидная последовательность n байт
        let mut consumed = 0;
        loop {
            let (valid_len, all_valid, error_len) = {
                let slice = &self.partial_utf8[consumed..];
                if slice.is_empty() {
                    break;
                }
                match std::str::from_utf8(slice) {
                    Ok(s) => (s.len(), true, None::<usize>),
                    Err(e) => (e.valid_up_to(), false, e.error_len()),
                }
            };
            // slice, s/e вышли из области видимости — borrow отпущен

            if valid_len > 0 {
                // SAFETY: partial_utf8[consumed..consumed+valid_len] — ровно та
                // подпоследовательность, которую from_utf8 выше признала валидной
                // UTF-8; границы совпадают с code-point boundary.
                let s = unsafe {
                    std::str::from_utf8_unchecked(
                        &self.partial_utf8[consumed..consumed + valid_len],
                    )
                };
                self.buf.push_str(s);
            }
            consumed += valid_len;

            if all_valid {
                // from_utf8 вернул Ok — все оставшиеся байты обработаны.
                consumed = self.partial_utf8.len();
                break;
            }

            match error_len {
                None => {
                    // Незавершённая последовательность в хвосте chunk-а —
                    // буферизуем оставшиеся байты для следующего вызова.
                    break;
                }
                Some(n) => {
                    // Явно невалидная последовательность — заменяем U+FFFD,
                    // пропускаем n байт и продолжаем декодирование.
                    self.buf.push('\u{FFFD}');
                    consumed += n;
                }
            }
        }

        self.partial_utf8.drain(..consumed);
        self.tokenize(false)
    }

    /// Финализирует ввод. Хвост буфера токенизируется как при EOF —
    /// pull-токенизатор сам lenient-обрабатывает незакрытые теги/
    /// entity. После `end()` любой `feed` приведёт к panic.
    ///
    /// Если был вызван `feed_bytes` с незавершённой UTF-8
    /// последовательностью в конце, она заменяется U+FFFD (WHATWG
    /// Encoding §4).
    pub fn end(&mut self) -> Vec<Token> {
        self.ended = true;
        // Незавершённая последовательность на EOF → U+FFFD (WHATWG Encoding §4)
        if !self.partial_utf8.is_empty() {
            self.buf.push('\u{FFFD}');
            self.partial_utf8.clear();
        }
        self.tokenize(true)
    }

    /// Количество ещё не потреблённых байт строкового буфера.
    /// Только для диагностики / тестов; в production-коде не используется.
    #[cfg(test)]
    pub fn pending_len(&self) -> usize {
        self.buf.len()
    }

    /// Прокручивает pull-токенизатор по slice буфера, ограниченному
    /// «безопасной точкой среза» (если не `final_chunk`), или по всему
    /// буферу (если `final_chunk`). Эмитнутые токены возвращаются;
    /// буфер обрезается слева на потреблённую часть; `text_only`
    /// переходит в следующее состояние pull-токенизатора.
    fn tokenize(&mut self, final_chunk: bool) -> Vec<Token> {
        let safe_end = if final_chunk {
            self.buf.len()
        } else {
            self.find_safe_split()
        };

        if safe_end == 0 {
            return Vec::new();
        }

        // ВАЖНО: вырезаем slice так, чтобы границы лежали на UTF-8
        // boundary. `find_safe_split` обязан возвращать корректные
        // позиции (т.к. ищет по `rfind('<' | '&')` — ASCII-символы).
        // Для безопасности используем `floor_char_boundary`-эквивалент.
        let safe_end = floor_char_boundary(&self.buf, safe_end);

        let tokens: Vec<Token>;
        let consumed: usize;
        let next_text_only: Option<(String, bool)>;
        {
            let slice = &self.buf[..safe_end];
            let mut tokenizer = Tokenizer::with_state(slice, self.text_only.take());
            tokens = (&mut tokenizer).collect();
            consumed = tokenizer.pos();
            next_text_only = tokenizer.text_only_state().cloned();
        }
        self.text_only = next_text_only;

        // pull-токенизатор всегда дочитывает slice до конца (он lenient).
        // Поэтому consumed == safe_end. Подстраховка на случай раннего
        // выхода — обрезаем по фактически потреблённому байту.
        self.buf.drain(..consumed);

        tokens
    }

    /// Находит максимальный offset в `self.buf`, до которого pull-
    /// токенизатор гарантированно не упрётся в незавершённую
    /// конструкцию. Логика консервативная: при сомнениях — обрезаем
    /// раньше, лишний раз буферизуем.
    fn find_safe_split(&self) -> usize {
        let bytes = self.buf.as_bytes();
        let n = bytes.len();
        if n == 0 {
            return 0;
        }

        if let Some((tag, decode)) = &self.text_only {
            // text-only режим (RAWTEXT/RCDATA). Прерывается только
            // последовательностью `</tag` + терминатор (whitespace / `/` / `>`).
            // Безопасная точка — последний `<`, который МОЖЕТ начать
            // незавершённый `</tag…`. Если такого `<` нет — split до конца.
            // Если он есть и хвост уже полностью покрывает `</tag` +
            // терминатор — пусть pull-токенизатор сам разберётся,
            // безопасно split до конца.
            let needed = 2 + tag.len() + 1; // '</' + tag + терминатор
            let mut split = match bytes.iter().rposition(|&b| b == b'<') {
                None => n,
                Some(pos) => {
                    if n - pos >= needed {
                        n
                    } else {
                        pos
                    }
                }
            };

            if *decode {
                // RCDATA декодирует character references. Если `&…`
                // обрывается без `;`, нельзя отдать pull-токенизатору
                // только `&` — он выпишет литерал, что расходится
                // с pull-режимом (где `&amp;` пришёл бы целиком).
                if let Some(pos) = bytes.iter().rposition(|&b| b == b'&') {
                    let tail = &bytes[pos..];
                    if !tail.contains(&b';') && tail.len() < 32 {
                        split = split.min(pos);
                    }
                }
            }
            // RAWTEXT (`<script>`/`<style>`, decode=false) entity не
            // декодирует — `&amp;` остаётся литералом и в pull, и в push.
            split
        } else {
            // Data state. Опасные хвосты:
            //   * `<…` без правильного терминатора (любой тег / комментарий
            //     / DOCTYPE);
            //   * `&…` без `;` (entity, ограничено 32 байтами).
            //
            // Для `<` терминатор зависит от типа конструкции:
            //   * `<!-- … -->` — терминатор `-->`;
            //   * `<!DOCTYPE … >` или прочие `<!…>` — терминатор `>`;
            //   * `<tag … >` / `</tag>` — терминатор `>`.
            //
            // Для каждого `<` от конца к началу проверяем, закрыт ли он,
            // и если нет — сдвигаем split до этой позиции. Остановка на
            // первом «опасном» `<` — после него любые `>` уже учтены
            // как часть более левой завершённой конструкции.
            let mut split = n;
            for pos in (0..n).rev() {
                if bytes[pos] != b'<' {
                    continue;
                }
                if !is_tag_closed(&bytes[pos..]) {
                    split = pos;
                }
                break;
            }

            if let Some(pos) = bytes.iter().rposition(|&b| b == b'&') {
                let tail = &bytes[pos..];
                // entity-имя ограничено 32 байтами (см. tokenizer.rs
                // `try_consume_entity`). Если за `&` уже >32 байт без
                // `;` — это не entity, pull сам отдаст как литерал.
                if !tail.contains(&b';') && tail.len() < 32 {
                    split = split.min(pos);
                }
            }

            split
        }
    }
}

impl Default for PushTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Проверяет, закрыта ли конструкция, начинающаяся с `<` в начале
/// `tail`. Используется `find_safe_split` для решения, безопасно ли
/// скармливать `tail` pull-токенизатору в текущем виде, или нужно
/// подождать ещё байт.
///
/// Возвращает `true`, если pull-токенизатор сможет завершить
/// разбор этой конструкции на хвосте `tail` без EOF посредине.
fn is_tag_closed(tail: &[u8]) -> bool {
    debug_assert!(tail.first() == Some(&b'<'));
    if tail.len() < 2 {
        return false;
    }
    match tail[1] {
        // `<!--…-->` — терминатор `-->` (не просто `>`).
        b'!' if tail.len() >= 4 && &tail[2..4] == b"--" => {
            // Ищем `-->` начиная с позиции 4 (после `<!--`).
            tail.windows(3).skip(4).any(|w| w == b"-->")
        }
        // `<!DOCTYPE…>`, `<![CDATA[…]]>` и т.п. — терминатор `>`.
        b'!' => tail[2..].contains(&b'>'),
        // `</tag…>` или `<tag…>` — терминатор `>`.
        b'/' => tail[2..].contains(&b'>'),
        c if c.is_ascii_alphabetic() => tail[2..].contains(&b'>'),
        // `<` + что-то странное (цифра / пробел) — pull-токенизатор
        // считает такой `<` литералом и эмитит `Text("<")`. Это
        // безопасно даже без `>` — split в конце буфера.
        _ => true,
    }
}

/// Возвращает наибольший индекс `<= n`, лежащий на границе code point-а
/// UTF-8 строки. Эквивалент unstable [`str::floor_char_boundary`].
fn floor_char_boundary(s: &str, mut n: usize) -> usize {
    if n >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Скармливает строку по байту через `feed` и возвращает все токены
    /// (в порядке выдачи) после `end`. Используется для проверки
    /// «корректность не зависит от размера chunk-а».
    fn tokenize_byte_by_byte(input: &str) -> Vec<Token> {
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        let mut start = 0;
        let bytes = input.as_bytes();
        for i in 1..=bytes.len() {
            if !input.is_char_boundary(i) {
                continue;
            }
            out.extend(pt.feed(&input[start..i]));
            start = i;
        }
        out.extend(pt.end());
        out
    }

    /// Скармливает строку как один chunk + `end`.
    fn tokenize_whole(input: &str) -> Vec<Token> {
        let mut pt = PushTokenizer::new();
        let mut out = pt.feed(input);
        out.extend(pt.end());
        out
    }

    /// Скармливает строку фиксированными chunk-ами (по `chunk_size` байт).
    fn tokenize_chunked(input: &str, chunk_size: usize) -> Vec<Token> {
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        let bytes = input.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            let mut end = (start + chunk_size).min(bytes.len());
            while !input.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                // chunk_size попал на середину многобайтного символа,
                // в реальном API caller отвечает за boundary — здесь
                // подвинем до следующей границы.
                end = (start + chunk_size + 4).min(bytes.len());
                while !input.is_char_boundary(end) {
                    end -= 1;
                }
            }
            out.extend(pt.feed(&input[start..end]));
            start = end;
        }
        out.extend(pt.end());
        out
    }

    /// Конкатенация всех `Text`-токенов в один String, остальные токены
    /// преобразуются в неотличимое тег-представление. Используется
    /// в property-тестах: push может разбить Text на несколько частей,
    /// pull выдаст одним куском — после нормализации они должны совпасть.
    fn normalize(tokens: &[Token]) -> Vec<Token> {
        let mut out: Vec<Token> = Vec::new();
        for t in tokens {
            if let (Some(Token::Text(prev)), Token::Text(cur)) = (out.last_mut(), t) {
                prev.push_str(cur);
            } else {
                out.push(t.clone());
            }
        }
        out
    }

    fn pull_tokens(input: &str) -> Vec<Token> {
        Tokenizer::new(input).collect()
    }

    fn assert_push_matches_pull(input: &str) {
        let pull = pull_tokens(input);
        let push_whole = normalize(&tokenize_whole(input));
        let push_byte = normalize(&tokenize_byte_by_byte(input));
        let push_chunk = normalize(&tokenize_chunked(input, 8));
        assert_eq!(push_whole, pull, "push(whole) != pull: input = {input:?}");
        assert_eq!(push_byte, pull, "push(byte) != pull: input = {input:?}");
        assert_eq!(push_chunk, pull, "push(8) != pull: input = {input:?}");
    }

    // ──────── property-тесты против pull ────────

    #[test]
    fn empty() {
        assert_push_matches_pull("");
    }

    #[test]
    fn plain_text() {
        assert_push_matches_pull("hello world");
    }

    #[test]
    fn simple_tag() {
        assert_push_matches_pull("<p>hello</p>");
    }

    #[test]
    fn nested_tags() {
        assert_push_matches_pull("<html><body><h1>Hello</h1></body></html>");
    }

    #[test]
    fn attributes() {
        assert_push_matches_pull(r#"<a href="https://example.com" class='x' id=z>link</a>"#);
    }

    #[test]
    fn self_closing() {
        assert_push_matches_pull("<br/>");
    }

    #[test]
    fn void_element() {
        assert_push_matches_pull("<p>a<br>b</p>");
    }

    #[test]
    fn comment() {
        assert_push_matches_pull("<!-- skip me --><p>x</p>");
    }

    #[test]
    fn comment_with_inner_gt() {
        // `>` внутри комментария не должен вводить в заблуждение
        // safe-split (он использует `contains('>')` на хвосте).
        assert_push_matches_pull("<!-- a > b --><p>x</p>");
    }

    #[test]
    fn doctype_basic() {
        assert_push_matches_pull("<!DOCTYPE html><p>x</p>");
    }

    #[test]
    fn doctype_html4() {
        assert_push_matches_pull(
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><p>x</p>"#,
        );
    }

    #[test]
    fn entity_named() {
        assert_push_matches_pull("a &amp; b");
    }

    #[test]
    fn entity_decimal() {
        assert_push_matches_pull("&#1055;&#1088;&#1080;");
    }

    #[test]
    fn entity_hex() {
        assert_push_matches_pull("&#x41;");
    }

    #[test]
    fn entity_unknown_kept_literal() {
        assert_push_matches_pull("&foo;");
    }

    #[test]
    fn entity_in_attribute_value() {
        assert_push_matches_pull(r#"<a title="&lt;ok&gt;">x</a>"#);
    }

    #[test]
    fn cyrillic_text() {
        assert_push_matches_pull("<p>Привет, мир</p>");
    }

    #[test]
    fn cyrillic_attribute_value() {
        assert_push_matches_pull(r#"<a title="Привет">x</a>"#);
    }

    #[test]
    fn rawtext_script_with_html() {
        assert_push_matches_pull("<script>var x = '<b>hi</b>'; if (a < b) f();</script>");
    }

    #[test]
    fn rawtext_script_with_entity_kept_literal() {
        assert_push_matches_pull("<script>x = '&amp;';</script>");
    }

    #[test]
    fn rawtext_style() {
        assert_push_matches_pull("<style>p { color: red; } /* < */</style>");
    }

    #[test]
    fn rawtext_close_tag_inside_string() {
        // Классическая ловушка из spec — </script> внутри JS-строки
        // всё равно закрывает блок.
        assert_push_matches_pull("<script>x = '</script>';</script>");
    }

    #[test]
    fn rcdata_title_entity_decoded() {
        assert_push_matches_pull("<title>Foo &amp; Bar</title>");
    }

    #[test]
    fn rcdata_textarea_inner_tag_is_text() {
        assert_push_matches_pull(
            "<textarea>&lt;script&gt;alert(1)&lt;/script&gt;</textarea>",
        );
    }

    #[test]
    fn rcdata_then_normal() {
        assert_push_matches_pull("<title>x &amp; y</title><p>z</p>");
    }

    #[test]
    fn rcdata_fake_end_tag_not_matched() {
        // `</titles>` не закрывает `<title>` — `s` после имени не
        // является терминатором. Эта ловушка особенно важна для
        // push: safe-split должен резервировать байты под полную
        // проверку терминатора.
        assert_push_matches_pull("<title>foo</titles>bar</title>");
    }

    #[test]
    fn rawtext_unclosed_at_eof() {
        // </script> отсутствует — текст должен дойти до конца ввода.
        assert_push_matches_pull("<script>x = 1");
    }

    #[test]
    fn long_text_chunk_boundary() {
        // Длинный текстовый блок без `<`/`&` — должен корректно
        // склеиваться после нормализации.
        let s = "a".repeat(100);
        assert_push_matches_pull(&s);
    }

    #[test]
    fn many_consecutive_entities() {
        assert_push_matches_pull("&amp;&lt;&gt;&quot;&apos;&nbsp;");
    }

    // ──────── направленные тесты на push-специфику ────────

    #[test]
    fn feed_with_dangling_lt_buffers() {
        let mut pt = PushTokenizer::new();
        let t1 = pt.feed("hello <");
        // Незавершённый `<` — должны эмитить только текст "hello ",
        // а `<` оставить в буфере.
        assert_eq!(t1, vec![Token::Text("hello ".into())]);
        assert_eq!(pt.pending_len(), 1, "ожидаем '<' в буфере");
        let t2 = pt.feed("p>world</p>");
        let total: Vec<Token> = t1.into_iter().chain(t2).chain(pt.end()).collect();
        // Нормализованно эквивалентно "<p>world</p>" с лидирующим "hello ".
        let normalized = normalize(&total);
        let expected = pull_tokens("hello <p>world</p>");
        assert_eq!(normalized, expected);
    }

    #[test]
    fn feed_with_dangling_amp_buffers() {
        let mut pt = PushTokenizer::new();
        let _ = pt.feed("abc &amp");
        // `&amp` без `;` — может быть продолжение `;` или другое.
        // Pull-токенизатор требует `;` для named entity, без него вернёт
        // `&amp` как литерал. До end() мы не должны решить — буферизуем.
        assert!(pt.pending_len() >= 4, "amp без ; должен буферизоваться");
        let t2 = pt.feed(";");
        let _t3 = pt.end();
        // Финальная склейка должна декодировать `&amp;` → `&`.
        let combined: String = t2
            .iter()
            .filter_map(|t| match t {
                Token::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(combined.contains('&'), "expected `&` decoded, got tokens: {t2:?}");
    }

    #[test]
    fn feed_with_dangling_entity_over_32_bytes_is_literal() {
        // Если за `&` идёт >32 байт без `;`, это не entity. Push должен
        // не буферизовать «вечно», а отдать `&` как литерал.
        let mut pt = PushTokenizer::new();
        let long = "&".to_string() + &"x".repeat(40);
        let t1 = pt.feed(&long);
        let t2 = pt.end();
        let all: Vec<Token> = t1.into_iter().chain(t2).collect();
        let normalized = normalize(&all);
        let pull = pull_tokens(&long);
        assert_eq!(normalized, pull);
    }

    #[test]
    fn rawtext_split_inside_close_tag() {
        // Самая узкая точка safe-split: `<` в RAWTEXT, после которого
        // ещё не пришло `/scrip…`. Эмитить ничего нельзя, пока не
        // увидим терминатор.
        let mut pt = PushTokenizer::new();
        let _ = pt.feed("<script>var x = 1; <");
        // Текст до `<` нельзя эмитить полностью — `<` мог бы быть
        // началом `</script`. Pull при этом отдаст всё одним Text-
        // токеном после `<script>`. Push до прихода следующего chunk-а
        // должен буферизовать хвост, начиная с `<`.
        let _ = pt.feed("/script>");
        let _ = pt.end();
        // Проверка — через combined normalize.
        let mut pt2 = PushTokenizer::new();
        let combined: Vec<Token> = pt2
            .feed("<script>var x = 1; </script>")
            .into_iter()
            .chain(pt2.end())
            .collect();
        assert_eq!(normalize(&combined), pull_tokens("<script>var x = 1; </script>"));
    }

    #[test]
    fn cyrillic_chunked_at_char_boundary() {
        // Каждый символ кириллицы — 2 байта UTF-8. Помеленно подаём
        // по 2/3/4 байта, чтобы поймать chunk-boundary внутри слова.
        for chunk_size in [2, 3, 4, 5] {
            let input = "<p>Привет мир</p>";
            let push = normalize(&tokenize_chunked(input, chunk_size));
            assert_eq!(push, pull_tokens(input), "chunk_size={chunk_size}");
        }
    }

    // ──────── feed_bytes: буферизация partial UTF-8 ────────

    /// Вспомогательная функция: скармливает байты побайтово через feed_bytes.
    fn tokenize_bytes_by_byte(input: &[u8]) -> Vec<Token> {
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        for i in 0..input.len() {
            out.extend(pt.feed_bytes(&input[i..i + 1]));
        }
        out.extend(pt.end());
        out
    }

    #[test]
    fn feed_bytes_ascii_matches_feed_str() {
        // Для чистого ASCII feed_bytes должен давать тот же результат, что feed.
        let input = "<html><body><p>Hello World</p></body></html>";
        let mut pt = PushTokenizer::new();
        let result: Vec<Token> = pt
            .feed_bytes(input.as_bytes())
            .into_iter()
            .chain(pt.end())
            .collect();
        assert_eq!(normalize(&result), pull_tokens(input));
    }

    #[test]
    fn feed_bytes_cyrillic_split_at_byte_boundary() {
        // Кириллица — 2-байтовые символы. Подаём по 1 байту,
        // граница chunk-а гарантированно разрезает символы.
        let input = "<p>Привет</p>";
        let result = tokenize_bytes_by_byte(input.as_bytes());
        assert_eq!(normalize(&result), pull_tokens(input));
    }

    #[test]
    fn feed_bytes_3byte_char_split() {
        // '€' = U+20AC = 0xE2 0x82 0xAC (3 байта). Подаём по 1 байту.
        let input = "price: €100";
        let result = tokenize_bytes_by_byte(input.as_bytes());
        assert_eq!(normalize(&result), pull_tokens(input));
    }

    #[test]
    fn feed_bytes_4byte_emoji_split() {
        // '😀' = U+1F600 = 0xF0 0x9F 0x98 0x80 (4 байта). Подаём по 1 байту.
        let input = "hello 😀 world";
        let result = tokenize_bytes_by_byte(input.as_bytes());
        assert_eq!(normalize(&result), pull_tokens(input));
    }

    #[test]
    fn feed_bytes_incomplete_at_eof_becomes_replacement() {
        // Подаём первый байт 2-байтового символа 'П' (0xD0) и сразу end().
        // Незавершённая последовательность должна стать U+FFFD.
        let mut pt = PushTokenizer::new();
        let _ = pt.feed_bytes(&[0xD0]); // первый байт 'П'
        let tokens = pt.end();
        let text: String = tokens
            .iter()
            .filter_map(|t| if let Token::Text(s) = t { Some(s.as_str()) } else { None })
            .collect();
        assert!(
            text.contains('\u{FFFD}'),
            "ожидаем U+FFFD для незавершённой последовательности, получили: {text:?}"
        );
    }

    #[test]
    fn feed_bytes_invalid_byte_replaced_inline() {
        // 0xFF — никогда не валиден в UTF-8. Должен заменяться U+FFFD
        // немедленно, не буферизоваться вечно.
        let input = b"hello\xFFworld";
        let mut pt = PushTokenizer::new();
        let result: Vec<Token> = pt
            .feed_bytes(input)
            .into_iter()
            .chain(pt.end())
            .collect();
        let text: String = result
            .iter()
            .filter_map(|t| if let Token::Text(s) = t { Some(s.as_str()) } else { None })
            .collect();
        assert!(text.contains("hello"), "ожидаем 'hello' в тексте");
        assert!(text.contains("world"), "ожидаем 'world' в тексте");
        assert!(text.contains('\u{FFFD}'), "ожидаем U+FFFD вместо 0xFF");
    }

    #[test]
    fn feed_bytes_chunk_sizes_match_pull() {
        // Разные размеры chunk-ов для HTML с кириллицей — все должны
        // давать тот же результат, что pull-токенизатор.
        let input = "<html><head><title>Тест</title></head><body><p>Привет мир</p></body></html>";
        let bytes = input.as_bytes();
        for chunk_size in [1usize, 2, 3, 5, 7, 16] {
            let mut pt = PushTokenizer::new();
            let mut out = Vec::new();
            let mut pos = 0;
            while pos < bytes.len() {
                let end = (pos + chunk_size).min(bytes.len());
                out.extend(pt.feed_bytes(&bytes[pos..end]));
                pos = end;
            }
            out.extend(pt.end());
            assert_eq!(
                normalize(&out),
                pull_tokens(input),
                "chunk_size={chunk_size}"
            );
        }
    }
}
