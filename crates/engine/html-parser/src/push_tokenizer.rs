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
    /// [`Tokenizer::cdata_allowed`][crate::tokenizer::Tokenizer], перенесённое
    /// между chunk-ами так же, как `text_only` (GAP-XMLDOC срез 15,
    /// BUG-685) — только `*_with_context` вызывающие (tree builder) реально
    /// меняют его через `on_token`'s второй элемент возврата; `feed`/
    /// `feed_bytes`/`end` без контекста (используются `preload_scanner`,
    /// которому namespace безразличен) держат его всегда `false`, что
    /// совпадает с их прежним поведением.
    cdata_allowed: bool,
    /// [`Tokenizer::xml_mode`][crate::tokenizer::Tokenizer] (GAP-XMLDOC срез
    /// 21, BUG-786) — в отличие от `cdata_allowed`, не меняется по ходу
    /// разбора, поэтому переносится в каждый внутренний `Tokenizer`
    /// напрямую из этого поля, не через `on_token`.
    xml_mode: bool,
}

impl PushTokenizer {
    /// Создаёт новый `PushTokenizer` в исходном состоянии.
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            text_only: None,
            ended: false,
            partial_utf8: Vec::new(),
            cdata_allowed: false,
            xml_mode: false,
        }
    }

    /// Взводит [`xml_mode`][Self::xml_mode] — зовётся один раз, документ
    /// либо XML-flavoured целиком, либо нет.
    pub fn set_xml_mode(&mut self, xml_mode: bool) {
        self.xml_mode = xml_mode;
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
        let mut out = Vec::new();
        self.tokenize(false, |tok| {
            out.push(tok);
            (false, false)
        });
        out
    }

    /// Вариант [`feed`][Self::feed], в который вызывающий (tree builder)
    /// подмешан прямо в цикл токенизации, а не применяет токены к дереву
    /// уже после того, как токенизатор дошёл до конца безопасного chunk-а
    /// (GAP-XMLDOC срез 12, BUG-685). Это принципиально: RAWTEXT/RCDATA
    /// решение (`is_raw_text_element`/`is_rcdata_element` в `tokenizer.rs`)
    /// принимается токенизатором *в момент* выдачи `StartTag`, до которого
    /// namespace ещё не известен — `on_token` вызывается сразу после
    /// каждого токена, пока внутренний `Tokenizer` жив, поэтому у
    /// вызывающего есть шанс аннулировать text-only через
    /// [`Tokenizer::cancel_text_only`] прежде, чем тот успеет
    /// просканировать хоть байт как RAWTEXT. `on_token` возвращает
    /// `(cancel_text_only, cdata_allowed)`: `cancel_text_only` — `true`,
    /// если text-only, только что выставленный ЭТИМ токеном (не
    /// self-closing `StartTag`), нужно отменить; `cdata_allowed` — станет
    /// ли следующий `<![CDATA[` реальной CDATA-секцией, пересчитанное
    /// после применения ЭТОГО токена — то же «пере-выводить после каждого
    /// токена», что `run_pull` делает в pull-режиме (GAP-XMLDOC срез 15,
    /// BUG-685: до этого среза push-режим никогда не взводил
    /// `cdata_allowed`, поэтому `<![CDATA[` в потоковой foreign content
    /// всегда уходил в bogus-comment ветку).
    pub fn feed_with_context(&mut self, chunk: &str, on_token: impl FnMut(Token) -> (bool, bool)) {
        assert!(!self.ended, "feed_with_context() after end()");
        self.buf.push_str(chunk);
        self.tokenize(false, on_token);
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
        self.decode_bytes_into_buf(chunk);
        let mut out = Vec::new();
        self.tokenize(false, |tok| {
            out.push(tok);
            (false, false)
        });
        out
    }

    /// [`feed_bytes`][Self::feed_bytes] variant of
    /// [`feed_with_context`][Self::feed_with_context] — see its docs.
    pub fn feed_bytes_with_context(
        &mut self,
        chunk: &[u8],
        on_token: impl FnMut(Token) -> (bool, bool),
    ) {
        self.decode_bytes_into_buf(chunk);
        self.tokenize(false, on_token);
    }

    /// UTF-8-decoding half of `feed_bytes`, shared with
    /// [`feed_bytes_with_context`][Self::feed_bytes_with_context] — everything
    /// up to (not including) the actual tokenization pass.
    fn decode_bytes_into_buf(&mut self, chunk: &[u8]) {
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
    }

    /// Финализирует ввод. Хвост буфера токенизируется как при EOF —
    /// pull-токенизатор сам lenient-обрабатывает незакрытые теги/
    /// entity. После `end()` любой `feed` приведёт к panic.
    ///
    /// Если был вызван `feed_bytes` с незавершённой UTF-8
    /// последовательностью в конце, она заменяется U+FFFD (WHATWG
    /// Encoding §4).
    pub fn end(&mut self) -> Vec<Token> {
        self.finalize_buf();
        let mut out = Vec::new();
        self.tokenize(true, |tok| {
            out.push(tok);
            (false, false)
        });
        out
    }

    /// [`end`][Self::end] variant of
    /// [`feed_with_context`][Self::feed_with_context] — see its docs.
    pub fn end_with_context(&mut self, on_token: impl FnMut(Token) -> (bool, bool)) {
        self.finalize_buf();
        self.tokenize(true, on_token);
    }

    fn finalize_buf(&mut self) {
        self.ended = true;
        // Незавершённая последовательность на EOF → U+FFFD (WHATWG Encoding §4)
        if !self.partial_utf8.is_empty() {
            self.buf.push('\u{FFFD}');
            self.partial_utf8.clear();
        }
    }

    /// Количество ещё не потреблённых байт строкового буфера.
    /// Только для диагностики / тестов; в production-коде не используется.
    #[cfg(test)]
    pub fn pending_len(&self) -> usize {
        self.buf.len()
    }

    /// Прокручивает pull-токенизатор по slice буфера, ограниченному
    /// «безопасной точкой среза» (если не `final_chunk`), или по всему
    /// буферу (если `final_chunk`). Каждый выданный токен сразу проходит
    /// через `on_token`, а не собирается в `Vec` заранее (GAP-XMLDOC срез
    /// 12, BUG-685) — `on_token` может отменить RAWTEXT/RCDATA, которое
    /// `Tokenizer` только что выставил внутри себя для этого самого токена
    /// (см. [`Tokenizer::cancel_text_only`]), и должна успеть сделать это
    /// до того, как тот же `tokenizer` продолжит собственный `next()` и
    /// начнёт сканировать text-only. Раньше здесь стоял один `.collect()`
    /// на весь безопасный slice — весь chunk токенизировался целиком, ДО
    /// того как вызывающий увидел хотя бы один токен, так что аннулировать
    /// решение было уже некому. Буфер обрезается слева на потреблённую
    /// часть; `text_only` переходит в следующее состояние pull-токенизатора.
    fn tokenize(&mut self, final_chunk: bool, mut on_token: impl FnMut(Token) -> (bool, bool)) {
        let safe_end = if final_chunk {
            self.buf.len()
        } else {
            self.find_safe_split()
        };

        if safe_end == 0 {
            return;
        }

        // ВАЖНО: вырезаем slice так, чтобы границы лежали на UTF-8
        // boundary. `find_safe_split` обязан возвращать корректные
        // позиции (т.к. ищет по `rfind('<' | '&')` — ASCII-символы).
        // Для безопасности используем `floor_char_boundary`-эквивалент.
        let safe_end = floor_char_boundary(&self.buf, safe_end);

        let consumed: usize;
        let next_text_only: Option<(String, bool)>;
        // GAP-XMLDOC срез 15 (BUG-685): переносится между chunk-ами так же,
        // как `text_only` — обновляется на самом токенизаторе после КАЖДОГО
        // токена (тот же приём, что `run_pull` для pull-режима), не только
        // между вызовами `tokenize`.
        let mut cdata_allowed = self.cdata_allowed;
        {
            let slice = &self.buf[..safe_end];
            let mut tokenizer = Tokenizer::with_state(slice, self.text_only.take());
            tokenizer.set_xml_mode(self.xml_mode);
            tokenizer.set_cdata_allowed(cdata_allowed);
            while let Some(tok) = tokenizer.next() {
                let opened_text_only =
                    matches!(&tok, Token::StartTag { self_closing: false, .. });
                let (cancel_text_only, next_cdata_allowed) = on_token(tok);
                if cancel_text_only && opened_text_only {
                    tokenizer.cancel_text_only();
                }
                cdata_allowed = next_cdata_allowed;
                tokenizer.set_cdata_allowed(cdata_allowed);
            }
            consumed = tokenizer.pos();
            next_text_only = tokenizer.text_only_state().cloned();
        }
        self.text_only = next_text_only;
        self.cdata_allowed = cdata_allowed;

        // pull-токенизатор всегда дочитывает slice до конца (он lenient).
        // Поэтому consumed == safe_end. Подстраховка на случай раннего
        // выхода — обрезаем по фактически потреблённому байту.
        self.buf.drain(..consumed);
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

        if let Some((_tag, decode)) = &self.text_only {
            // text-only режим (RAWTEXT/RCDATA). Прерывается только
            // последовательностью `</tag` + терминатор (whitespace / `/` / `>`).
            // Безопасная точка — последний `<`, который МОЖЕТ начать
            // незавершённый `</tag…`. Если такого `<` нет — split до конца.
            //
            // GAP-XMLDOC срез 29: раньше «закрыт ли он» проверялось только
            // длиной («хватает ли байт под форму `</tag>`+терминатор») —
            // не тем, что там ДЕЙСТВИТЕЛЬНО есть. Это ломалось, если
            // `</tag>` уже закрылся РАНЬШЕ последнего `<` в буфере, а сам
            // последний `<` — не он, а начало СЛЕДУЮЩЕЙ, ещё не закрытой
            // Data-state конструкции (`<?target d`, длиннее «needed» байт,
            // но без `?>`) — код решал «хватает места», хотя реального
            // терминатора там не было. `is_tag_closed` — та же проверка,
            // что уже используют Data-state-хвосты ниже; тег-агностична
            // (не сверяет имя с `tag`, только форму `</…>` вообще), что
            // тем же самым консервативным принципом модуля безопасно и
            // здесь: при сомнении подождать лишний байт не вредно.
            let mut split = match bytes.iter().rposition(|&b| b == b'<') {
                None => n,
                Some(pos) => {
                    if is_tag_closed(&bytes[pos..], self.xml_mode) {
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
        } else if let Some(open_pos) = last_unterminated_cdata_start(bytes) {
            // GAP-XMLDOC срез 15 (BUG-685): буквальный `<![CDATA[`, чей `]]>`
            // ещё не пришёл, занимает ВЕСЬ остаток буфера — CDATA-контент
            // сам может содержать `<`/`>` (это данные, а не новые
            // конструкции), поэтому обычный «последний `<`» скан ниже сюда
            // заходить не должен: он принял бы внутренний `<` за начало
            // нового тега/комментария и мог бы посчитать буфер «закрытым»
            // раньше настоящего `]]>` (пример — `<![CDATA[a<b]]>c`: без
            // этой ветки последний `<` в буфере — тот, что внутри `a<b`, а
            // не в `<![CDATA[`). Ждать `]]>` всегда безопасно — вопрос
            // прежний: срез 15 не срезает по первому `>` внутри секции.
            open_pos
        } else if let Some(open_pos) = last_unterminated_doctype_subset_start(bytes, self.xml_mode) {
            // GAP-XMLDOC срез 29: то же нарушение допущения, что чинила
            // ветка CDATA выше, только у другого XML-mode-специфичного
            // вложенного `<...>` (срез 21) — `<!DOCTYPE html [<?PI?>]>`'s
            // internal subset can hold its own `<?target data?>`/
            // `<!--comment-->`, which the scan below's "check only the
            // LAST `<` in the buffer" shortcut would find fully closed on
            // its own (its `?>`/`-->` arrived) while the outer DOCTYPE is
            // still waiting for `]>` — the shortcut would then never look
            // back at the DOCTYPE's own still-open `<` at all.
            open_pos
        } else {
            // Data state. Опасные хвосты:
            //   * `<…` без правильного терминатора (любой тег / комментарий
            //     / DOCTYPE — включая уже терминированный `<![CDATA[…]]>`,
            //     который сюда и не попадает благодаря ветке выше);
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
                if !is_tag_closed(&bytes[pos..], self.xml_mode) {
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

/// Ищет байтовое смещение первого литерального (case-sensitive) `<![CDATA[`
/// в `bytes`, чей `]]>` ещё не пришёл — GAP-XMLDOC срез 15 (BUG-685),
/// используется `find_safe_split`. Уже терминированные секции пропускаются
/// (поиск продолжается сразу после их `]]>`), поэтому
/// `<![CDATA[x]]><![CDATA[y` корректно возвращает позицию ВТОРОГО маркера,
/// а не первого.
fn last_unterminated_cdata_start(bytes: &[u8]) -> Option<usize> {
    const MARKER: &[u8] = b"<![CDATA[";
    const TERMINATOR: &[u8] = b"]]>";
    let mut pos = 0;
    while pos + MARKER.len() <= bytes.len() {
        let rel = bytes[pos..].windows(MARKER.len()).position(|w| w == MARKER)?;
        let start = pos + rel;
        let content_start = start + MARKER.len();
        match bytes[content_start..].windows(TERMINATOR.len()).position(|w| w == TERMINATOR) {
            Some(term_rel) => {
                // Секция уже терминирована — продолжаем поиск за её `]]>`.
                pos = content_start + term_rel + TERMINATOR.len();
            }
            None => return Some(start),
        }
    }
    None
}

/// Case-insensitive ASCII substring search — `needle_lower` must already be
/// lowercase. Same narrowing as `xml_entities::find_ci`, reimplemented here
/// (over `&[u8]`, not `&str`) since `feed_bytes` may call this before a
/// chunk boundary lands on a UTF-8 code point boundary.
fn find_ci_ascii(haystack: &[u8], needle_lower: &[u8]) -> Option<usize> {
    if needle_lower.is_empty() || haystack.len() < needle_lower.len() {
        return None;
    }
    haystack.windows(needle_lower.len()).position(|w| w.eq_ignore_ascii_case(needle_lower))
}

/// Finds a `<!DOCTYPE ... [` in `xml_mode` whose internal subset (and the
/// DOCTYPE's own closing `>` after it) has not fully arrived in `bytes` yet
/// — GAP-XMLDOC срез 29. `find_safe_split`'s generic scan below only ever
/// checks the LAST `<` in the buffer, which is safe for plain HTML (no
/// top-level construct nests another) but not for a DOCTYPE internal
/// subset (срез 21): a `<?PI?>`/`<!--comment-->` *inside* `[ ... ]` is
/// itself a nested `<...>` construct that can look fully closed (its own
/// `?>`/`-->` arrived) while the outer DOCTYPE is still open, the same
/// nesting problem [`last_unterminated_cdata_start`] already solves for
/// `<![CDATA[`. `false` from `xml_mode` means every `<!DOCTYPE` here is
/// plain HTML5 bogus-DOCTYPE (stops at the first `>`, no subset concept at
/// all) — same behaviour as before this срез.
fn last_unterminated_doctype_subset_start(bytes: &[u8], xml_mode: bool) -> Option<usize> {
    if !xml_mode {
        return None;
    }
    const KEYWORD: &[u8] = b"<!doctype";
    let mut pos = 0;
    while pos < bytes.len() {
        let rel = find_ci_ascii(&bytes[pos..], KEYWORD)?;
        let start = pos + rel;
        if !doctype_bang_closed(&bytes[start..], true) {
            return Some(start);
        }
        pos = start + KEYWORD.len();
    }
    None
}

/// Проверяет, закрыта ли конструкция, начинающаяся с `<` в начале
/// `tail`. Используется `find_safe_split` для решения, безопасно ли
/// скармливать `tail` pull-токенизатору в текущем виде, или нужно
/// подождать ещё байт.
///
/// Возвращает `true`, если pull-токенизатор сможет завершить
/// разбор этой конструкции на хвосте `tail` без EOF посредине.
///
/// Литеральный (case-sensitive) `<![CDATA[` всегда ждёт `]]>`, независимо
/// от того, допустима ли сейчас реальная CDATA-секция (GAP-XMLDOC срез 15,
/// BUG-685) — токен, который делает её допустимой (открывающий `<svg>`/
/// `<math>`), может обнаружиться позже в ТОМ ЖЕ буфере, до которого этот
/// статический анализ не заглядывает; ждать дольше здесь всегда безопасно
/// (тот же консервативный принцип, что у `find_safe_split` в целом).
fn is_tag_closed(tail: &[u8], xml_mode: bool) -> bool {
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
        // Литеральный `<![CDATA[…]]>` — терминатор `]]>`, не первый `>`.
        // `starts_with` уже требует `tail.len() >= 9`, поэтому неполный
        // префикс (`<![CDA`) падает в generic-ветку ниже, что верно: без
        // `>` вообще она и так не «закрыта».
        b'!' if tail.starts_with(b"<![CDATA[") => tail[9..].windows(3).any(|w| w == b"]]>"),
        // `<!DOCTYPE…>` и прочее — терминатор `>` (GAP-XMLDOC срез 29:
        // но не самый ПЕРВЫЙ `>`, если это DOCTYPE с internal subset —
        // см. `doctype_bang_closed`).
        b'!' => doctype_bang_closed(tail, xml_mode),
        // `</tag…>` или `<tag…>` — терминатор `>`.
        b'/' => tail[2..].contains(&b'>'),
        c if c.is_ascii_alphabetic() => tail[2..].contains(&b'>'),
        // `<?target data?>` — GAP-XMLDOC срез 29. Terminator depends on what
        // the real tokenizer does with it: in `xml_mode` it's a real
        // processing instruction (срез 23) or, for the `target == "xml"`
        // case, an XML-declaration bogus comment (срез 22's redirect inside
        // срез 23's `consume_processing_instruction`) — either way the scan
        // stops at `?>`, since a leading `<?xml …?>` also always ends in
        // `?>`. Outside `xml_mode`, `<?` unconditionally becomes an ordinary
        // bogus comment (срез 22, not gated on `xml_mode`), whose terminator
        // is a lone `>` like any other bogus comment.
        b'?' if xml_mode => tail.windows(2).any(|w| w == b"?>"),
        b'?' => tail[1..].contains(&b'>'),
        // `<` + что-то странное (цифра / пробел) — pull-токенизатор
        // считает такой `<` литералом и эмитит `Text("<")`. Это
        // безопасно даже без `>` — split в конце буфера.
        _ => true,
    }
}

/// Terminator check for `<!DOCTYPE…>`/other bogus `<!…>` markup, aware of a
/// DOCTYPE internal subset (`<!DOCTYPE html [ ... ]>`) in `xml_mode`
/// (GAP-XMLDOC срез 29) — mirrors the balanced-`<...>` walk
/// `Tokenizer::consume_doctype` (срез 21) and `xml_entities::parse_declared_entities`
/// already do: a `<?PI?>`/`<!--comment-->` inside the subset may contain its
/// own `>`, which must not be mistaken for the subset's (or the DOCTYPE's)
/// end. Outside `xml_mode` — or when there is no `[` at all — the terminator
/// is simply the first `>`, same as before this срез.
fn doctype_bang_closed(tail: &[u8], xml_mode: bool) -> bool {
    let body = &tail[2..];
    if xml_mode && let Some(bracket_rel) = body.iter().position(|&b| b == b'[') {
        let mut depth: i32 = 0;
        let mut i = bracket_rel + 1;
        while i < body.len() {
            match body[i] {
                b'<' => depth += 1,
                b'>' if depth > 0 => depth -= 1,
                b']' if depth == 0 => return body[i + 1..].contains(&b'>'),
                _ => {}
            }
            i += 1;
        }
        return false;
    }
    body.contains(&b'>')
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

    // ──────── CDATA в потоковом foreign content (GAP-XMLDOC срез 15) ────────

    /// Обновляет `inside_svg` по StartTag/EndTag `<svg>` в `tok` — общая
    /// замена для настоящего tree builder-а в этих тестах: `cdata_allowed`
    /// становится `true` ровно тогда, когда становится `true`
    /// `IncrementalTreeBuilder::cdata_sections_allowed` для документа с
    /// одним foreign-элементом верхнего уровня.
    fn track_svg(tok: &Token, inside_svg: &mut bool) {
        match tok {
            Token::StartTag { name, self_closing: false, .. } if name == "svg" => {
                *inside_svg = true;
            }
            Token::EndTag { name } if name == "svg" => *inside_svg = false,
            _ => {}
        }
    }

    /// Токенизирует `input` через `feed_with_context`/`end_with_context`,
    /// c `cdata_allowed`, реалистично привязанным к тому, открыт ли сейчас
    /// `<svg>` (GAP-XMLDOC срез 15, BUG-685) — так же, как
    /// `IncrementalTreeBuilder::apply_token_for_stream` привязывает его к
    /// `cdata_sections_allowed`.
    fn tokenize_svg_scoped_cdata_chunked(input: &str, chunk_size: usize) -> Vec<Token> {
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        let mut inside_svg = false;
        let bytes = input.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let end = (pos + chunk_size).min(bytes.len());
            pt.feed_with_context(&input[pos..end], |tok| {
                track_svg(&tok, &mut inside_svg);
                let cdata_allowed = inside_svg;
                out.push(tok);
                (false, cdata_allowed)
            });
            pos = end;
        }
        pt.end_with_context(|tok| {
            track_svg(&tok, &mut inside_svg);
            let cdata_allowed = inside_svg;
            out.push(tok);
            (false, cdata_allowed)
        });
        out
    }

    /// Pull-эквивалент [`tokenize_svg_scoped_cdata_chunked`] — то же
    /// «взвести перед каждым `next()`», что `run_pull` делает через
    /// `IncrementalTreeBuilder::cdata_sections_allowed`.
    fn pull_tokens_svg_scoped_cdata(input: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(input);
        let mut inside_svg = false;
        let mut out = Vec::new();
        loop {
            t.set_cdata_allowed(inside_svg);
            match t.next() {
                Some(tok) => {
                    track_svg(&tok, &mut inside_svg);
                    out.push(tok);
                }
                None => break,
            }
        }
        out
    }

    #[test]
    fn cdata_inside_svg_opened_in_the_same_stream_matches_pull() {
        // GAP-XMLDOC срез 15 (BUG-685): `<svg>` opens and the CDATA section
        // both arrive over the network — `cdata_allowed` must flip to `true`
        // in response to the `<svg>` StartTag, exactly like pull, not stay
        // stuck at the stale value from before this chunk.
        let input = "<svg><![CDATA[a<b]]>c</svg>";
        // `normalize` — потому что pull может само по себе разбить смежные
        // Text-токены на несколько (CDATA-контент и последующий Data-текст
        // — разные emission-события даже в pull), и push волен резать их
        // по-своему на chunk boundary; тождественность гарантируется на
        // уровне text-node coalescing в tree builder-е (см. модульный
        // докстринг `tree_builder.rs`), не на уровне сырых токенов.
        let pull = normalize(&pull_tokens_svg_scoped_cdata(input));
        for chunk_size in [1usize, 2, 3, 5, 100] {
            let push = normalize(&tokenize_svg_scoped_cdata_chunked(input, chunk_size));
            assert_eq!(push, pull, "chunk_size={chunk_size}");
        }
    }

    #[test]
    fn cdata_with_gt_before_terminator_survives_chunk_split_right_after_gt() {
        // Regression: a bare `>` inside CDATA content, landing right at a
        // chunk boundary, must not be mistaken for the section's terminator
        // by `find_safe_split` — only `]]>` ends a real CDATA section.
        // Before срез 15's CDATA-aware `is_tag_closed` branch, the generic
        // `<!...>` case used `contains(&b'>')`, so this exact split would
        // have looked "safe" and truncated the section early. The `<svg>`
        // opens in the SAME first chunk as the truncation point, which is
        // exactly the case the stale-`self.cdata_allowed` heuristic can't
        // see coming — hence `is_tag_closed` must ignore it entirely.
        let input = "<svg><![CDATA[x>y]]>z</svg>";
        let split = input.find("x>y").unwrap() + 2; // сразу после "x>"
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        let mut inside_svg = false;
        pt.feed_with_context(&input[..split], |tok| {
            track_svg(&tok, &mut inside_svg);
            let cdata_allowed = inside_svg;
            out.push(tok);
            (false, cdata_allowed)
        });
        pt.feed_with_context(&input[split..], |tok| {
            track_svg(&tok, &mut inside_svg);
            let cdata_allowed = inside_svg;
            out.push(tok);
            (false, cdata_allowed)
        });
        pt.end_with_context(|tok| {
            track_svg(&tok, &mut inside_svg);
            let cdata_allowed = inside_svg;
            out.push(tok);
            (false, cdata_allowed)
        });
        assert_eq!(normalize(&out), normalize(&pull_tokens_svg_scoped_cdata(input)));
    }

    #[test]
    fn cdata_disallowed_context_still_becomes_bogus_comment_when_streamed() {
        // `on_token` reports `cdata_allowed = false` throughout (adjusted
        // current node never foreign) — streamed `<![CDATA[` must fall back
        // to the same bogus-comment shape as pull, not silently vanish.
        let input = "<![CDATA[ignore]]><p>x</p>";
        let mut pt = PushTokenizer::new();
        let mut out = Vec::new();
        pt.feed_with_context(input, |tok| {
            out.push(tok);
            (false, false)
        });
        pt.end_with_context(|tok| {
            out.push(tok);
            (false, false)
        });
        assert_eq!(normalize(&out), pull_tokens(input));
    }

    #[test]
    fn xml_mode_pi_right_after_rawtext_close_is_not_split_by_length_heuristic() {
        // GAP-XMLDOC срез 29: `find_safe_split`'s RAWTEXT/RCDATA branch used
        // to decide "safe" purely by whether enough trailing bytes existed
        // after the buffer's LAST `<` to fit a `</tag>`-shaped pattern — not
        // whether that tail was actually closed. Here the true `</style>`
        // closes early in the buffer, and the LAST `<` is instead the start
        // of a still-unterminated `<?target d` (long enough to satisfy the
        // old length check, `?>` never arrived) — the old heuristic handed
        // the whole buffer to the pull tokenizer anyway, which (lenient,
        // reads its slice to the end) truncated the PI at EOF instead of
        // waiting for more bytes.
        let input = concat!(
            "<html><body><style><![CDATA[a]]></style>",
            "<?target data?></body></html>",
        );
        let mut pt = PushTokenizer::new();
        pt.set_xml_mode(true);
        let split_at = input.find("<?target d").unwrap() + "<?target d".len();
        let (head, tail) = input.split_at(split_at);
        let mut out = pt.feed(head);
        out.extend(pt.feed(tail));
        out.extend(pt.end());
        assert!(
            out.iter().any(
                |t| matches!(t, Token::ProcessingInstruction { target, data }
                    if target == "target" && data == "data")
            ),
            "PI must not be truncated at the chunk boundary: {out:?}"
        );
    }
}

