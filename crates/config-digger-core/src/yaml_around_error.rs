//! The YAML parser VS Code reads a prompt file's frontmatter with,
//! `src/vs/base/common/yaml.ts`, the same in 1.129.1 and 1.141.0 [binary].
//! It never fails: it reads what it can and goes on, so a frontmatter that
//! is not YAML still gives VS Code the values around what it cannot read.
//! This is a port of its scanner and parser as far as the values go: the
//! errors it collects are dropped, as VS Code drops them when it loads a
//! skill. Offsets count characters where VS Code counts UTF-16 code units,
//! which differ only in a column after a character outside the BMP.

/// A value as VS Code's parser reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    /// A scalar, which VS Code reads as a string whatever it holds.
    Scalar(String),
    /// A sequence, block or flow, whose items config-digger does not need.
    Sequence,
    /// A mapping, block or flow, each key with its value in order, a key
    /// given twice kept twice.
    Map(Vec<(String, Node)>),
}

/// The value VS Code's parser reads from `input`, or none when it holds
/// nothing but blank lines, comments and document markers.
pub(crate) fn parse(input: &str) -> Option<Node> {
    let input: Vec<char> = input.chars().collect();
    let tokens = Scanner::new(&input).scan();
    Parser {
        tokens: &tokens,
        input: &input,
        pos: 0,
    }
    .parse()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scalar,
    Colon,
    Dash,
    Comma,
    FlowMapStart,
    FlowMapEnd,
    FlowSeqStart,
    FlowSeqEnd,
    Newline,
    Indent,
    Comment,
    DocumentStart,
    DocumentEnd,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    start: usize,
    /// For a scalar, its value as read; empty for any other token.
    value: String,
    /// Whether it is a plain scalar, or another token: not quoted and no
    /// `|` or `>` block scalar.
    plain: bool,
    /// For an indent, its width.
    indent: i64,
}

impl Token {
    fn new(kind: Kind, start: usize) -> Token {
        Token {
            kind,
            start,
            value: String::new(),
            plain: true,
            indent: 0,
        }
    }

    fn scalar(start: usize, value: String, plain: bool) -> Token {
        Token {
            value,
            plain,
            ..Token::new(Kind::Scalar, start)
        }
    }
}

fn is_line_end(c: Option<char>) -> bool {
    matches!(c, Some('\n' | '\r'))
}

fn is_blank_or_end(c: Option<char>) -> bool {
    matches!(c, None | Some(' ' | '\t' | '\n' | '\r'))
}

struct Scanner<'i> {
    input: &'i [char],
    pos: usize,
    tokens: Vec<Token>,
    /// How deep inside flow collections the scanner is, across lines.
    flow_depth: usize,
    /// Whether a block `:` came on the current line, after which a `: `
    /// is part of a scalar.
    seen_block_colon: bool,
    seen_document_start: usize,
}

impl<'i> Scanner<'i> {
    fn new(input: &'i [char]) -> Scanner<'i> {
        Scanner {
            input,
            pos: 0,
            tokens: Vec::new(),
            flow_depth: 0,
            seen_block_colon: false,
            seen_document_start: 0,
        }
    }

    fn at(&self, pos: usize) -> Option<char> {
        self.input.get(pos).copied()
    }

    fn peek(&self) -> Option<char> {
        self.at(self.pos)
    }

    fn push(&mut self, kind: Kind, start: usize) {
        self.tokens.push(Token::new(kind, start));
    }

    fn scan(mut self) -> Vec<Token> {
        while self.pos < self.input.len() {
            self.scan_line();
            if self.seen_document_start > 1 {
                break;
            }
        }
        self.push(Kind::Eof, self.pos);
        self.tokens
    }

    fn scan_line(&mut self) {
        self.seen_block_colon = false;
        if self.peek() == Some('\n') {
            self.push(Kind::Newline, self.pos);
            self.pos += 1;
            return;
        }
        if self.peek() == Some('\r') {
            self.push(Kind::Newline, self.pos);
            self.pos += if self.at(self.pos + 1) == Some('\n') {
                2
            } else {
                1
            };
            return;
        }
        let indent_start = self.pos;
        let mut indent = 0;
        while matches!(self.peek(), Some(' ' | '\t')) {
            indent += 1;
            self.pos += 1;
        }
        if indent > 0 {
            self.tokens.push(Token {
                indent,
                ..Token::new(Kind::Indent, indent_start)
            });
        }
        if self.pos >= self.input.len() || is_line_end(self.peek()) {
            self.scan_newline();
            return;
        }
        if indent == 0 && self.input.len() - self.pos >= 3 {
            let marker: String = self.input[self.pos..self.pos + 3].iter().collect();
            if (marker == "---" || marker == "...") && is_blank_or_end(self.at(self.pos + 3)) {
                let kind = if marker == "---" {
                    Kind::DocumentStart
                } else {
                    Kind::DocumentEnd
                };
                self.push(kind, self.pos);
                self.pos += 3;
                self.scan_line_content();
                self.scan_newline();
                if kind == Kind::DocumentStart {
                    self.seen_document_start += 1;
                }
                return;
            }
        }
        if self.peek() == Some('#') {
            self.scan_comment();
            self.scan_newline();
            return;
        }
        // A directive line, such as `%YAML 1.2`.
        if self.peek() == Some('%') {
            while self.pos < self.input.len() && !is_line_end(self.peek()) {
                self.pos += 1;
            }
            self.scan_newline();
            return;
        }
        self.scan_line_content();
        self.scan_newline();
    }

    fn scan_line_content(&mut self) {
        while self.pos < self.input.len() && !is_line_end(self.peek()) {
            self.skip_inline_whitespace();
            let Some(ch) = self.peek().filter(|c| !matches!(c, '\n' | '\r')) else {
                break;
            };
            let in_flow = self.flow_depth > 0;
            match ch {
                '#' => {
                    self.scan_comment();
                    break;
                }
                '{' | '[' => {
                    self.flow_depth += 1;
                    let kind = if ch == '{' {
                        Kind::FlowMapStart
                    } else {
                        Kind::FlowSeqStart
                    };
                    self.push(kind, self.pos);
                    self.pos += 1;
                }
                '}' | ']' if in_flow => {
                    self.flow_depth -= 1;
                    let kind = if ch == '}' {
                        Kind::FlowMapEnd
                    } else {
                        Kind::FlowSeqEnd
                    };
                    self.push(kind, self.pos);
                    self.pos += 1;
                }
                ',' if in_flow => {
                    self.push(Kind::Comma, self.pos);
                    self.pos += 1;
                }
                '-' if is_blank_or_end(self.at(self.pos + 1)) => {
                    self.push(Kind::Dash, self.pos);
                    self.pos += 1;
                }
                ':' if self.is_block_colon() => {
                    self.push(Kind::Colon, self.pos);
                    self.pos += 1;
                    if !in_flow {
                        self.seen_block_colon = true;
                    }
                }
                ':' if in_flow && self.last_token_is_json_like() => {
                    self.push(Kind::Colon, self.pos);
                    self.pos += 1;
                }
                '\'' | '"' => self.scan_quoted_scalar(ch),
                '|' | '>' if !in_flow && self.is_block_scalar_start() => {
                    self.scan_block_scalar(ch);
                    break;
                }
                _ => self.scan_unquoted_scalar(),
            }
        }
    }

    /// Whether the `:` here is a mapping value indicator.
    fn is_block_colon(&self) -> bool {
        if self.seen_block_colon && self.flow_depth == 0 {
            return false;
        }
        let next = self.at(self.pos + 1);
        is_blank_or_end(next) || (self.flow_depth > 0 && matches!(next, Some(',' | '}' | ']')))
    }

    /// Whether the last token but blanks and comments is a quoted scalar or
    /// the end of a flow collection.
    fn last_token_is_json_like(&self) -> bool {
        self.tokens
            .iter()
            .rev()
            .find(|t| !matches!(t.kind, Kind::Newline | Kind::Indent | Kind::Comment))
            .is_some_and(|t| {
                (t.kind == Kind::Scalar && !t.plain)
                    || matches!(t.kind, Kind::FlowMapEnd | Kind::FlowSeqEnd)
            })
    }

    fn scan_quoted_scalar(&mut self, quote: char) {
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        // Trailing blanks of the source, which folding trims, unlike those
        // an escape gives.
        let mut trailing_blanks = 0;
        while let Some(ch) = self.peek() {
            if ch == quote {
                if quote == '\'' && self.at(self.pos + 1) == Some('\'') {
                    value.push('\'');
                    self.pos += 2;
                    trailing_blanks = 0;
                    continue;
                }
                self.pos += 1;
                self.tokens.push(Token::scalar(start, value, false));
                return;
            }
            if quote == '"' && ch == '\\' {
                let next = self.at(self.pos + 1);
                if is_line_end(next) {
                    self.pos += 1;
                    self.consume_newline();
                    self.skip_inline_whitespace();
                    trailing_blanks = 0;
                    continue;
                }
                let hex = |digits: usize, this: &Scanner| -> Option<u32> {
                    let text: String = this
                        .input
                        .get(this.pos + 2..this.pos + 2 + digits)?
                        .iter()
                        .collect();
                    parse_int_hex(&text)
                };
                let escaped = match next {
                    Some('n') => Some('\n'),
                    Some('t') => Some('\t'),
                    Some('\\') => Some('\\'),
                    Some('"') => Some('"'),
                    Some('/') => Some('/'),
                    Some('r') => Some('\r'),
                    Some('0') => Some('\0'),
                    Some('a') => Some('\x07'),
                    Some('b') => Some('\x08'),
                    Some('e') => Some('\x1b'),
                    Some('v') => Some('\x0b'),
                    Some('f') => Some('\x0c'),
                    Some(' ') => Some(' '),
                    Some('_') => Some('\u{a0}'),
                    _ => None,
                };
                if let Some(escaped) = escaped {
                    value.push(escaped);
                    self.pos += 2;
                } else if let Some((letter, digits)) = match next {
                    Some('x') => Some(('x', 2)),
                    Some('u') => Some(('u', 4)),
                    Some('U') => Some(('U', 8)),
                    _ => None,
                } {
                    match hex(digits, self) {
                        Some(code) => {
                            value.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            self.pos += 2 + digits;
                        }
                        None => {
                            value.push('\\');
                            value.push(letter);
                            self.pos += 2;
                        }
                    }
                } else {
                    value.push('\\');
                    value.extend(next);
                    self.pos += 2;
                }
                trailing_blanks = 0;
                continue;
            }
            if ch == '\n' || ch == '\r' {
                value.truncate(value.len() - trailing_blanks);
                trailing_blanks = 0;
                self.consume_newline();
                let mut empty_lines = 0;
                while self.pos < self.input.len() {
                    self.skip_inline_whitespace();
                    if is_line_end(self.peek()) {
                        empty_lines += 1;
                        self.consume_newline();
                    } else {
                        break;
                    }
                }
                if empty_lines > 0 {
                    value.push_str(&"\n".repeat(empty_lines));
                } else {
                    value.push(' ');
                }
                continue;
            }
            if ch == ' ' || ch == '\t' {
                trailing_blanks += 1;
            } else {
                trailing_blanks = 0;
            }
            value.push(ch);
            self.pos += 1;
        }
        // A quote that never closes holds the rest of the input.
        self.tokens.push(Token::scalar(start, value, false));
    }

    fn scan_unquoted_scalar(&mut self) {
        let start = self.pos;
        let mut end = self.pos;
        while let Some(ch) = self.peek() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            if self.flow_depth > 0 && matches!(ch, ',' | '}' | ']' | '{' | '[') {
                break;
            }
            if ch == ':' && self.is_block_colon() {
                break;
            }
            if ch == '#' && self.pos > start && matches!(self.at(self.pos - 1), Some(' ' | '\t')) {
                break;
            }
            self.pos += 1;
            if ch != ' ' && ch != '\t' {
                end = self.pos;
            }
        }
        let value = self.input[start..end].iter().collect();
        self.tokens.push(Token::scalar(start, value, true));
    }

    /// Whether the `|` or `>` here opens a block scalar: only indicators,
    /// blanks and a comment follow it on its line.
    fn is_block_scalar_start(&self) -> bool {
        let mut p = self.pos + 1;
        while matches!(self.at(p), Some('1'..='9' | '+' | '-')) {
            p += 1;
        }
        while matches!(self.at(p), Some(' ' | '\t')) {
            p += 1;
        }
        matches!(self.at(p), None | Some('\n' | '\r' | '#'))
    }

    fn scan_block_scalar(&mut self, style: char) {
        let start = self.pos;
        self.pos += 1;
        let mut explicit_indent = 0;
        #[derive(PartialEq)]
        enum Chomping {
            Clip,
            Strip,
            Keep,
        }
        let mut chomping = Chomping::Clip;
        for _ in 0..2 {
            match self.peek() {
                Some(c @ '1'..='9') if explicit_indent == 0 => {
                    explicit_indent = i64::from(c.to_digit(10).unwrap_or(0));
                    self.pos += 1;
                }
                Some('-') if chomping == Chomping::Clip => {
                    chomping = Chomping::Strip;
                    self.pos += 1;
                }
                Some('+') if chomping == Chomping::Clip => {
                    chomping = Chomping::Keep;
                    self.pos += 1;
                }
                _ => {}
            }
        }
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
        if self.peek() == Some('#') {
            while self.pos < self.input.len() && !is_line_end(self.peek()) {
                self.pos += 1;
            }
        }
        self.consume_newline();
        let parent_indent = self.parent_block_indent();
        let mut content_indent = if explicit_indent > 0 {
            parent_indent + explicit_indent
        } else {
            0
        };
        let mut lines: Vec<String> = Vec::new();
        let mut trailing_newlines = 0;
        while self.pos < self.input.len() {
            let line_start = self.pos;
            let mut line_indent: i64 = 0;
            while self.peek() == Some(' ') {
                line_indent += 1;
                self.pos += 1;
            }
            if self.pos >= self.input.len() || is_line_end(self.peek()) {
                if content_indent > 0 && line_indent >= content_indent {
                    let from = line_start + content_indent as usize;
                    let preserved: String = self.input[from..self.pos].iter().collect();
                    if preserved.is_empty() {
                        trailing_newlines += 1;
                    } else {
                        trailing_newlines = 0;
                    }
                    lines.push(preserved);
                } else {
                    lines.push(String::new());
                    trailing_newlines += 1;
                }
                self.consume_newline();
                continue;
            }
            if line_indent == 0 && self.input.len() - self.pos >= 3 {
                let marker: String = self.input[self.pos..self.pos + 3].iter().collect();
                if (marker == "---" || marker == "...") && is_blank_or_end(self.at(self.pos + 3)) {
                    self.pos = line_start;
                    break;
                }
            }
            if content_indent == 0 {
                if line_indent <= parent_indent {
                    self.pos = line_start;
                    break;
                }
                content_indent = line_indent;
            }
            if line_indent < content_indent {
                self.pos = line_start;
                break;
            }
            let content_start = line_start + content_indent as usize;
            while self.pos < self.input.len() && !is_line_end(self.peek()) {
                self.pos += 1;
            }
            lines.push(self.input[content_start..self.pos].iter().collect());
            trailing_newlines = 0;
            self.consume_newline();
        }
        let mut value = if style == '|' {
            lines.join("\n")
        } else {
            fold_block(&lines)
        };
        if trailing_newlines > 0 {
            let kept = value.trim_end_matches('\n').len();
            value.truncate(kept);
        }
        let has_content = lines.iter().any(|line| !line.is_empty());
        match chomping {
            Chomping::Clip if has_content => value.push('\n'),
            Chomping::Clip | Chomping::Strip => {}
            Chomping::Keep if has_content => value.push_str(&"\n".repeat(trailing_newlines + 1)),
            Chomping::Keep => value = "\n".repeat(trailing_newlines),
        }
        self.tokens.push(Token::scalar(start, value, false));
    }

    /// The indentation of the block a block scalar starts in: the column
    /// of its mapping key or of its sequence dash, or -1 at the top level.
    fn parent_block_indent(&self) -> i64 {
        let mut before = self
            .tokens
            .iter()
            .rev()
            .filter(|t| !matches!(t.kind, Kind::Newline | Kind::Comment | Kind::Indent));
        match before.next() {
            Some(colon) if colon.kind == Kind::Colon => before
                .next()
                .map_or(0, |key| column_at(self.input, key.start)),
            Some(dash) if dash.kind == Kind::Dash => column_at(self.input, dash.start),
            Some(start) if start.kind == Kind::DocumentStart => -1,
            _ => 0,
        }
    }

    fn scan_comment(&mut self) {
        let start = self.pos;
        while self.pos < self.input.len() && !is_line_end(self.peek()) {
            self.pos += 1;
        }
        self.push(Kind::Comment, start);
    }

    fn scan_newline(&mut self) {
        let start = self.pos;
        if self.consume_newline() {
            self.push(Kind::Newline, start);
        }
    }

    fn skip_inline_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn consume_newline(&mut self) -> bool {
        match self.peek() {
            Some('\r') if self.at(self.pos + 1) == Some('\n') => self.pos += 2,
            Some('\n' | '\r') => self.pos += 1,
            _ => return false,
        }
        true
    }
}

/// The column of `offset`: how far it is from the start of its line.
fn column_at(input: &[char], offset: usize) -> i64 {
    input[..offset]
        .iter()
        .rev()
        .take_while(|c| !matches!(c, '\n' | '\r'))
        .count() as i64
}

/// The leading hex digits of `text` as JavaScript's `parseInt(text, 16)`
/// reads them, or none when it starts with none.
fn parse_int_hex(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_hexdigit).collect();
    u32::from_str_radix(&digits, 16).ok()
}

/// The lines of a `>` block scalar folded as VS Code folds them: a break
/// between two lines that are not more indented becomes a space, and each
/// empty line a line break.
fn fold_block(lines: &[String]) -> String {
    let mut value = String::new();
    let mut last_more_indented = false;
    let mut in_empty_run = false;
    let mut seen_content = false;
    for (index, line) in lines.iter().enumerate() {
        let more_indented = line.starts_with([' ', '\t']);
        if line.is_empty() {
            value.push('\n');
            in_empty_run = true;
            continue;
        }
        if index == 0 {
            value = line.clone();
        } else if in_empty_run {
            if (last_more_indented || more_indented) && seen_content {
                value.push('\n');
            }
            value.push_str(line);
            in_empty_run = false;
        } else if more_indented || last_more_indented {
            value.push('\n');
            value.push_str(line);
        } else {
            value.push(' ');
            value.push_str(line);
        }
        last_more_indented = more_indented;
        seen_content = true;
    }
    value
}

struct Parser<'t> {
    tokens: &'t [Token],
    input: &'t [char],
    pos: usize,
}

impl Parser<'_> {
    fn parse(&mut self) -> Option<Node> {
        self.skip_newlines_and_comments();
        if self.kind() == Kind::DocumentStart {
            self.advance();
            self.skip_newlines_and_comments();
        }
        if matches!(self.kind(), Kind::Eof | Kind::DocumentEnd) {
            return None;
        }
        self.parse_value(-1)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn kind(&self) -> Kind {
        self.current().kind
    }

    fn peek(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let token = self.current().clone();
        if token.kind != Kind::Eof {
            self.pos += 1;
        }
        token
    }

    /// The current token, consumed when it is of `kind`.
    fn expect(&mut self, kind: Kind) -> Token {
        if self.kind() == kind {
            self.advance()
        } else {
            self.current().clone()
        }
    }

    fn skip_newlines_and_comments(&mut self) {
        loop {
            let blank = match self.kind() {
                Kind::Newline | Kind::Comment => true,
                Kind::Indent => {
                    matches!(self.peek(1).kind, Kind::Newline | Kind::Comment | Kind::Eof)
                }
                _ => false,
            };
            if !blank {
                break;
            }
            self.advance();
        }
    }

    fn current_indent(&self) -> i64 {
        if self.kind() == Kind::Indent {
            self.current().indent
        } else {
            0
        }
    }

    fn peek_past_indent(&self) -> &Token {
        if self.kind() == Kind::Indent {
            self.peek(1)
        } else {
            self.current()
        }
    }

    fn looks_like_mapping(&self) -> bool {
        let offset = usize::from(self.kind() == Kind::Indent);
        self.peek(offset).kind == Kind::Scalar && self.peek(offset + 1).kind == Kind::Colon
    }

    fn column_of(&self, token: &Token) -> i64 {
        column_at(self.input, token.start)
    }

    fn parse_value(&mut self, parent_indent: i64) -> Option<Node> {
        self.skip_newlines_and_comments();
        let kind = self.kind();
        let flow = if kind == Kind::Indent {
            self.peek(1).kind
        } else {
            kind
        };
        if matches!(flow, Kind::FlowMapStart | Kind::FlowSeqStart) {
            if kind == Kind::Indent {
                self.advance();
            }
            return Some(if flow == Kind::FlowMapStart {
                self.parse_flow_map()
            } else {
                self.parse_flow_seq()
            });
        }
        let indent = self.current_indent();
        if self.peek_past_indent().kind == Kind::Dash {
            return Some(self.parse_block_sequence(indent));
        }
        if self.looks_like_mapping() {
            return Some(self.parse_block_mapping(indent, false));
        }
        if matches!(kind, Kind::Scalar | Kind::Indent) {
            return Some(self.parse_scalar(parent_indent));
        }
        None
    }

    fn parse_scalar(&mut self, parent_indent: i64) -> Node {
        if self.kind() == Kind::Indent {
            self.advance();
        }
        let token = self.expect(Kind::Scalar);
        if !token.plain {
            return Node::Scalar(token.value);
        }
        self.parse_plain_multiline(token, parent_indent)
    }

    /// A plain scalar whose first line is `first`, going on over the lines
    /// indented deeper than `parent_indent`.
    fn parse_plain_multiline(&mut self, first: Token, parent_indent: i64) -> Node {
        let mut value = first.value;
        loop {
            let saved = self.pos;
            let mut empty_lines = 0;
            let mut found = false;
            while self.kind() == Kind::Newline {
                self.advance();
                match self.kind() {
                    Kind::Newline => {
                        empty_lines += 1;
                        continue;
                    }
                    Kind::Indent => match self.peek(1).kind {
                        Kind::Newline | Kind::Eof => {
                            empty_lines += 1;
                            self.advance();
                            continue;
                        }
                        Kind::Comment => {}
                        _ => found = self.current().indent > parent_indent,
                    },
                    Kind::Eof | Kind::DocumentStart | Kind::DocumentEnd => {}
                    _ => found = parent_indent < 0,
                }
                break;
            }
            if !found {
                self.pos = saved;
                break;
            }
            if self.kind() == Kind::Indent {
                self.advance();
            }
            let separator = if empty_lines > 0 {
                "\n".repeat(empty_lines)
            } else {
                " ".to_owned()
            };
            if self.kind() != Kind::Scalar {
                // A deeper `-` is text, not a sequence.
                if self.kind() == Kind::Dash {
                    self.advance();
                    let mut line = "-".to_owned();
                    if self.kind() == Kind::Scalar {
                        line = format!("- {}", self.advance().value);
                    }
                    value.push_str(&separator);
                    value.push_str(&line);
                    continue;
                }
                self.pos = saved;
                break;
            }
            // A key starts the next entry.
            if self.peek(1).kind == Kind::Colon {
                self.pos = saved;
                break;
            }
            let next = self.advance();
            value.push_str(&separator);
            value.push_str(&next.value);
        }
        Node::Scalar(value)
    }

    fn parse_block_mapping(&mut self, base_indent: i64, inline_first_entry: bool) -> Node {
        let mut properties = Vec::new();
        if inline_first_entry && let Some(entry) = self.parse_mapping_entry(base_indent) {
            properties.push(entry);
        }
        while self.kind() != Kind::Eof {
            self.skip_newlines_and_comments();
            if self.kind() == Kind::Eof {
                break;
            }
            // A deeper key is read as one of this mapping, VS Code noting
            // the indentation as an error.
            if self.current_indent() < base_indent || !self.looks_like_mapping() {
                break;
            }
            let Some(entry) = self.parse_mapping_entry(base_indent) else {
                break;
            };
            properties.push(entry);
        }
        Node::Map(properties)
    }

    fn parse_mapping_entry(&mut self, base_indent: i64) -> Option<(String, Node)> {
        if self.kind() == Kind::Indent {
            self.advance();
        }
        let key = self.expect(Kind::Scalar).value;
        if self.expect(Kind::Colon).kind != Kind::Colon {
            return None;
        }
        let value = self.parse_mapping_value(base_indent);
        Some((key, value))
    }

    fn parse_mapping_value(&mut self, base_indent: i64) -> Node {
        match self.kind() {
            Kind::FlowMapStart => return self.parse_flow_map(),
            Kind::FlowSeqStart => return self.parse_flow_seq(),
            Kind::Scalar => {
                let token = self.advance();
                if !token.plain {
                    return Node::Scalar(token.value);
                }
                return self.parse_plain_multiline(token, base_indent);
            }
            _ => {}
        }
        self.skip_newlines_and_comments();
        if self.kind() == Kind::Eof {
            return empty_scalar();
        }
        let next_indent = self.current_indent();
        // A sequence at the key's own indentation is its value.
        if next_indent == base_indent && self.peek_past_indent().kind == Kind::Dash {
            return self.parse_value(base_indent).unwrap_or_else(empty_scalar);
        }
        if next_indent <= base_indent {
            return empty_scalar();
        }
        self.parse_value(base_indent).unwrap_or_else(empty_scalar)
    }

    fn parse_block_sequence(&mut self, base_indent: i64) -> Node {
        let mut first = true;
        while self.kind() != Kind::Eof {
            self.skip_newlines_and_comments();
            if self.kind() == Kind::Eof {
                break;
            }
            let indent = if first && self.kind() == Kind::Dash {
                self.column_of(self.current())
            } else {
                self.current_indent()
            };
            first = false;
            if indent < base_indent || self.peek_past_indent().kind != Kind::Dash {
                break;
            }
            if self.kind() == Kind::Indent {
                self.advance();
            }
            self.advance();
            self.parse_sequence_item_value(base_indent);
        }
        Node::Sequence
    }

    fn parse_sequence_item_value(&mut self, base_indent: i64) -> Node {
        let next = self.current().clone();
        if next.kind == Kind::Comment {
            self.advance();
        }
        match next.kind {
            Kind::FlowMapStart => return self.parse_flow_map(),
            Kind::FlowSeqStart => return self.parse_flow_seq(),
            Kind::Dash => {
                let nested = self.column_of(&next);
                return self.parse_block_sequence(nested);
            }
            Kind::Scalar => {
                if self.peek(1).kind == Kind::Colon {
                    let item_indent = self.column_of(&next);
                    return self.parse_block_mapping(item_indent, true);
                }
                return self.parse_scalar(base_indent);
            }
            _ => {}
        }
        self.skip_newlines_and_comments();
        if self.kind() == Kind::Eof || self.current_indent() <= base_indent {
            return empty_scalar();
        }
        self.parse_value(base_indent).unwrap_or_else(empty_scalar)
    }

    fn parse_flow_map(&mut self) -> Node {
        self.advance();
        let mut properties = Vec::new();
        self.skip_flow_whitespace();
        while !matches!(self.kind(), Kind::FlowMapEnd | Kind::Eof) {
            if self.kind() != Kind::Scalar {
                break;
            }
            let key = self.flow_scalar();
            self.skip_flow_whitespace();
            let value = if self.kind() == Kind::Colon {
                self.advance();
                self.skip_flow_whitespace();
                self.parse_flow_value()
            } else {
                empty_scalar()
            };
            properties.push((key, value));
            self.skip_flow_whitespace();
            if self.kind() == Kind::Comma {
                self.advance();
                self.skip_flow_whitespace();
            }
        }
        if self.kind() == Kind::FlowMapEnd {
            self.advance();
        }
        Node::Map(properties)
    }

    fn parse_flow_seq(&mut self) -> Node {
        self.advance();
        self.skip_flow_whitespace();
        while !matches!(self.kind(), Kind::FlowSeqEnd | Kind::Eof) {
            match self.kind() {
                Kind::FlowMapStart => {
                    self.parse_flow_map();
                }
                Kind::FlowSeqStart => {
                    self.parse_flow_seq();
                }
                Kind::Scalar => {
                    self.flow_scalar();
                }
                _ => {
                    self.advance();
                    continue;
                }
            }
            self.skip_flow_whitespace();
            if self.kind() == Kind::Comma {
                self.advance();
                self.skip_flow_whitespace();
            }
        }
        if self.kind() == Kind::FlowSeqEnd {
            self.advance();
        }
        Node::Sequence
    }

    /// A scalar in a flow collection, a plain one folded over the plain
    /// scalars of the lines after it.
    fn flow_scalar(&mut self) -> String {
        let token = self.advance();
        if !token.plain {
            return token.value;
        }
        let mut value = token.value;
        loop {
            let mut newline = false;
            let mut p = self.pos;
            while let Some(t) = self.tokens.get(p) {
                match t.kind {
                    Kind::Newline => newline = true,
                    Kind::Indent | Kind::Comment => {}
                    _ => break,
                }
                p += 1;
            }
            match self.tokens.get(p) {
                Some(next) if newline && next.kind == Kind::Scalar && next.plain => {
                    value.push(' ');
                    value.push_str(&next.value);
                    self.pos = p + 1;
                }
                _ => break,
            }
        }
        value
    }

    fn parse_flow_value(&mut self) -> Node {
        match self.kind() {
            Kind::FlowMapStart => self.parse_flow_map(),
            Kind::FlowSeqStart => self.parse_flow_seq(),
            Kind::Scalar => Node::Scalar(self.flow_scalar()),
            _ => empty_scalar(),
        }
    }

    fn skip_flow_whitespace(&mut self) {
        while matches!(self.kind(), Kind::Newline | Kind::Indent | Kind::Comment) {
            self.advance();
        }
    }
}

/// The empty scalar VS Code gives a key without a value.
fn empty_scalar() -> Node {
    Node::Scalar(String::new())
}
