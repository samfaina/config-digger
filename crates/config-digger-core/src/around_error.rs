//! JSONC read around its errors, as VS Code reads some of its files: a port
//! of the `parse` of `node-jsonc-parser` 3.3.1, which skips what it cannot
//! use and goes on, keeping what parses on both sides of an error.

use serde_json::Value;

/// What a reader keeps of a text it reads around its errors.
#[derive(Debug, PartialEq)]
pub(crate) struct Recovered {
    /// The value kept, `None` when the text holds none at all.
    pub value: Option<Value>,
    /// The byte offset of the first error, `None` when the text parses.
    pub first_error: Option<usize>,
}

/// `text` read as `node-jsonc-parser`'s `parse(text, errors, { allowTrailingComma })`
/// reads it: comments allowed, and a trailing comma too with
/// `allow_trailing_comma`.
pub(crate) fn parse(text: &str, allow_trailing_comma: bool) -> Recovered {
    let mut parser = Parser {
        scanner: Scanner::new(text),
        allow_trailing_comma,
        first_error: None,
        values: Values::default(),
    };
    parser.scan_next();
    if parser.scanner.token == Token::Eof {
        parser.error();
    } else {
        if !parser.parse_value() {
            parser.error();
        }
        if parser.scanner.token != Token::Eof {
            parser.error();
        }
    }
    Recovered {
        value: parser.values.root.into_iter().next(),
        first_error: parser.first_error,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Colon,
    Comma,
    Null,
    True,
    False,
    String,
    Number,
    LineComment,
    BlockComment,
    LineBreak,
    Trivia,
    Unknown,
    Eof,
}

/// The scanner of `node-jsonc-parser`, over the bytes of the text: every
/// character it tells apart is ASCII, so a token never ends inside another
/// character.
struct Scanner<'t> {
    text: &'t str,
    pos: usize,
    token: Token,
    token_offset: usize,
    /// The value of a string or number token.
    value: String,
    /// Whether the token just scanned holds an error.
    error: bool,
}

impl<'t> Scanner<'t> {
    fn new(text: &'t str) -> Self {
        Scanner {
            text,
            pos: 0,
            token: Token::Unknown,
            token_offset: 0,
            value: String::new(),
            error: false,
        }
    }

    fn at(&self, pos: usize) -> Option<u8> {
        self.text.as_bytes().get(pos).copied()
    }

    fn scan(&mut self) -> Token {
        self.token = self.scan_token();
        self.token
    }

    fn scan_token(&mut self) -> Token {
        self.value.clear();
        self.error = false;
        self.token_offset = self.pos;
        let Some(code) = self.at(self.pos) else {
            self.token_offset = self.text.len();
            return Token::Eof;
        };
        if is_white_space(code) {
            while self.at(self.pos).is_some_and(is_white_space) {
                self.pos += 1;
            }
            return Token::Trivia;
        }
        if is_line_break(code) {
            self.pos += 1;
            if code == b'\r' && self.at(self.pos) == Some(b'\n') {
                self.pos += 1;
            }
            return Token::LineBreak;
        }
        let single = match code {
            b'{' => Some(Token::OpenBrace),
            b'}' => Some(Token::CloseBrace),
            b'[' => Some(Token::OpenBracket),
            b']' => Some(Token::CloseBracket),
            b':' => Some(Token::Colon),
            b',' => Some(Token::Comma),
            _ => None,
        };
        if let Some(token) = single {
            self.pos += 1;
            return token;
        }
        match code {
            b'"' => {
                self.pos += 1;
                self.value = self.scan_string();
                Token::String
            }
            b'/' if self.at(self.pos + 1) == Some(b'/') => {
                self.pos += 2;
                while self.at(self.pos).is_some_and(|byte| !is_line_break(byte)) {
                    self.pos += 1;
                }
                Token::LineComment
            }
            b'/' if self.at(self.pos + 1) == Some(b'*') => {
                self.pos += 2;
                let mut closed = false;
                while self.pos + 1 < self.text.len() {
                    if self.at(self.pos) == Some(b'*') && self.at(self.pos + 1) == Some(b'/') {
                        self.pos += 2;
                        closed = true;
                        break;
                    }
                    self.pos += 1;
                }
                if !closed {
                    self.pos = self.text.len();
                    self.error = true;
                }
                Token::BlockComment
            }
            b'-' if !self
                .at(self.pos + 1)
                .is_some_and(|byte| byte.is_ascii_digit()) =>
            {
                self.pos += 1;
                Token::Unknown
            }
            b'-' | b'0'..=b'9' => {
                if code == b'-' {
                    self.pos += 1;
                }
                let end = self.scan_number();
                self.value = self.text[self.token_offset..end].to_owned();
                Token::Number
            }
            _ => {
                // A word: a keyword, or else an unknown symbol.
                while self.at(self.pos).is_some_and(is_unknown_content) {
                    self.pos += 1;
                }
                if self.pos == self.token_offset {
                    self.pos += 1;
                    return Token::Unknown;
                }
                match &self.text[self.token_offset..self.pos] {
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::Null,
                    _ => Token::Unknown,
                }
            }
        }
    }

    /// Scans the digits of a number, its sign scanned, up to where its
    /// value ends.
    fn scan_number(&mut self) -> usize {
        let digit = |scanner: &Self| scanner.at(scanner.pos).is_some_and(|b| b.is_ascii_digit());
        if self.at(self.pos) == Some(b'0') {
            self.pos += 1;
        } else {
            self.pos += 1;
            while digit(self) {
                self.pos += 1;
            }
        }
        if self.at(self.pos) == Some(b'.') {
            self.pos += 1;
            if !digit(self) {
                self.error = true;
                return self.pos;
            }
            while digit(self) {
                self.pos += 1;
            }
        }
        let mut end = self.pos;
        if matches!(self.at(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.at(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if digit(self) {
                while digit(self) {
                    self.pos += 1;
                }
                end = self.pos;
            } else {
                self.error = true;
            }
        }
        end
    }

    /// Scans a string after its opening quote, as UTF-16 code units the
    /// way JavaScript builds it, so that an escaped surrogate pair makes one
    /// character.
    fn scan_string(&mut self) -> String {
        let mut units: Vec<u16> = Vec::new();
        let mut start = self.pos;
        loop {
            let Some(ch) = self.at(self.pos) else {
                units.extend(self.text[start..self.pos].encode_utf16());
                self.error = true;
                break;
            };
            if ch == b'"' {
                units.extend(self.text[start..self.pos].encode_utf16());
                self.pos += 1;
                break;
            }
            if ch == b'\\' {
                units.extend(self.text[start..self.pos].encode_utf16());
                self.pos += 1;
                let Some(escaped) = self.text[self.pos..].chars().next() else {
                    self.error = true;
                    break;
                };
                self.pos += escaped.len_utf8();
                let unit = match escaped {
                    '"' => Some(u16::from(b'"')),
                    '\\' => Some(u16::from(b'\\')),
                    '/' => Some(u16::from(b'/')),
                    'b' => Some(0x08),
                    'f' => Some(0x0c),
                    'n' => Some(u16::from(b'\n')),
                    'r' => Some(u16::from(b'\r')),
                    't' => Some(u16::from(b'\t')),
                    'u' => self.scan_hex_digits(),
                    _ => None,
                };
                match unit {
                    Some(unit) => units.push(unit),
                    None => self.error = true,
                }
                start = self.pos;
                continue;
            }
            if ch <= 0x1f {
                if is_line_break(ch) {
                    units.extend(self.text[start..self.pos].encode_utf16());
                    self.error = true;
                    break;
                }
                // An error, but the string goes on.
                self.error = true;
            }
            self.pos += 1;
        }
        String::from_utf16_lossy(&units)
    }

    /// The code unit of the four hex digits of a `\u` escape, `None` when
    /// fewer follow, which are scanned all the same.
    fn scan_hex_digits(&mut self) -> Option<u16> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let digit = self
                .at(self.pos)
                .and_then(|byte| char::from(byte).to_digit(16))?;
            value = value * 16 + u16::try_from(digit).unwrap_or_default();
            self.pos += 1;
        }
        Some(value)
    }
}

fn is_white_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn is_line_break(byte: u8) -> bool {
    byte == b'\n' || byte == b'\r'
}

/// Whether `byte` goes on a word, such as a keyword.
fn is_unknown_content(byte: u8) -> bool {
    !is_white_space(byte) && !is_line_break(byte) && !b"}]{[\":,/".contains(&byte)
}

/// The number JavaScript's `Number` reads in the text of a number token, as
/// serde_json holds the same literal in valid JSON.
fn number(text: &str) -> Value {
    if let Ok(integer) = text.parse::<i64>() {
        return integer.into();
    }
    if let Ok(integer) = text.parse::<u64>() {
        return integer.into();
    }
    text.parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

/// The values `parse`'s visitor builds: each object or array goes into its
/// parent when it ends, at the property current when it began.
#[derive(Default)]
struct Values {
    /// The values at the top level; only the first counts.
    root: Vec<Value>,
    /// The open objects and arrays, each with the property it is under.
    open: Vec<(Option<String>, Value)>,
    property: Option<String>,
}

impl Values {
    fn add(&mut self, property: Option<String>, value: Value) {
        match self.open.last_mut() {
            None => self.root.push(value),
            Some((_, Value::Array(array))) => array.push(value),
            Some((_, Value::Object(object))) => {
                if let Some(property) = property {
                    object.insert(property, value);
                }
            }
            Some(_) => {}
        }
    }

    fn literal(&mut self, value: Value) {
        self.add(self.property.clone(), value);
    }

    fn begin(&mut self, container: Value) {
        self.open.push((self.property.take(), container));
    }

    fn end(&mut self) {
        if let Some((property, container)) = self.open.pop() {
            self.add(property, container);
        }
    }
}

struct Parser<'t> {
    scanner: Scanner<'t>,
    allow_trailing_comma: bool,
    first_error: Option<usize>,
    values: Values,
}

impl Parser<'_> {
    fn error(&mut self) {
        self.first_error.get_or_insert(self.scanner.token_offset);
    }

    /// Reports an error at the current token, then skips tokens up to one
    /// of `until`, or past one of `until_after`.
    fn handle_error(&mut self, until_after: &[Token], until: &[Token]) {
        self.error();
        if until_after.is_empty() && until.is_empty() {
            return;
        }
        let mut token = self.scanner.token;
        while token != Token::Eof {
            if until_after.contains(&token) {
                self.scan_next();
                break;
            }
            if until.contains(&token) {
                break;
            }
            token = self.scan_next();
        }
    }

    /// The next token that is no comment or whitespace, each scan error and
    /// unknown symbol on the way an error.
    fn scan_next(&mut self) -> Token {
        loop {
            let token = self.scanner.scan();
            if self.scanner.error {
                self.error();
            }
            match token {
                Token::LineComment | Token::BlockComment | Token::Trivia | Token::LineBreak => {}
                Token::Unknown => self.error(),
                _ => return token,
            }
        }
    }

    fn parse_value(&mut self) -> bool {
        match self.scanner.token {
            Token::OpenBracket => self.parse_array(),
            Token::OpenBrace => self.parse_object(),
            Token::String => {
                let value = std::mem::take(&mut self.scanner.value);
                self.values.literal(Value::String(value));
                self.scan_next();
                true
            }
            Token::Number => {
                let value = number(&self.scanner.value);
                self.values.literal(value);
                self.scan_next();
                true
            }
            Token::Null | Token::True | Token::False => {
                let value = match self.scanner.token {
                    Token::Null => Value::Null,
                    token => Value::Bool(token == Token::True),
                };
                self.values.literal(value);
                self.scan_next();
                true
            }
            _ => false,
        }
    }

    fn parse_property(&mut self) -> bool {
        if self.scanner.token != Token::String {
            self.handle_error(&[], &[Token::CloseBrace, Token::Comma]);
            return false;
        }
        self.values.property = Some(std::mem::take(&mut self.scanner.value));
        self.scan_next();
        if self.scanner.token == Token::Colon {
            self.scan_next();
            if !self.parse_value() {
                self.handle_error(&[], &[Token::CloseBrace, Token::Comma]);
            }
        } else {
            self.handle_error(&[], &[Token::CloseBrace, Token::Comma]);
        }
        true
    }

    fn parse_object(&mut self) -> bool {
        self.values.begin(Value::Object(serde_json::Map::new()));
        self.scan_next();
        let mut needs_comma = false;
        while !matches!(self.scanner.token, Token::CloseBrace | Token::Eof) {
            if self.scanner.token == Token::Comma {
                if !needs_comma {
                    self.error();
                }
                self.scan_next();
                if self.scanner.token == Token::CloseBrace && self.allow_trailing_comma {
                    break;
                }
            } else if needs_comma {
                self.error();
            }
            if !self.parse_property() {
                self.handle_error(&[], &[Token::CloseBrace, Token::Comma]);
            }
            needs_comma = true;
        }
        self.values.end();
        if self.scanner.token == Token::CloseBrace {
            self.scan_next();
        } else {
            self.handle_error(&[Token::CloseBrace], &[]);
        }
        true
    }

    fn parse_array(&mut self) -> bool {
        self.values.begin(Value::Array(Vec::new()));
        self.scan_next();
        let mut needs_comma = false;
        while !matches!(self.scanner.token, Token::CloseBracket | Token::Eof) {
            if self.scanner.token == Token::Comma {
                if !needs_comma {
                    self.error();
                }
                self.scan_next();
                if self.scanner.token == Token::CloseBracket && self.allow_trailing_comma {
                    break;
                }
            } else if needs_comma {
                self.error();
            }
            if !self.parse_value() {
                self.handle_error(&[], &[Token::CloseBracket, Token::Comma]);
            }
            needs_comma = true;
        }
        self.values.end();
        if self.scanner.token == Token::CloseBracket {
            self.scan_next();
        } else {
            self.handle_error(&[Token::CloseBracket], &[]);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    //! The parse cases of `node-jsonc-parser`'s own `json.test.ts`.

    use super::*;
    use serde_json::json;

    fn valid(text: &str, expected: Value, allow_trailing_comma: bool) {
        let recovered = parse(text, allow_trailing_comma);
        assert_eq!(recovered.first_error, None, "{text}");
        assert_eq!(recovered.value, Some(expected), "{text}");
    }

    fn invalid(text: &str, expected: Option<Value>, allow_trailing_comma: bool) {
        let recovered = parse(text, allow_trailing_comma);
        assert!(recovered.first_error.is_some(), "{text}");
        assert_eq!(recovered.value, expected, "{text}");
    }

    #[test]
    fn parse_literals() {
        valid("true", json!(true), false);
        valid("false", json!(false), false);
        valid("null", json!(null), false);
        valid(r#""foo""#, json!("foo"), false);
        valid(
            r#""\"-\\-\/-\b-\f-\n-\r-\t""#,
            json!("\"-\\-/-\u{8}-\u{c}-\n-\r-\t"),
            false,
        );
        valid(r#""Ü""#, json!("Ü"), false);
        valid("9", json!(9), false);
        valid("-9", json!(-9), false);
        valid("0.129", json!(0.129), false);
        valid("23e3", json!(23e3), false);
        valid("1.2E+3", json!(1.2E+3), false);
        valid("1.2E-3", json!(1.2E-3), false);
        valid("1.2E-3 // comment", json!(1.2E-3), false);
    }

    #[test]
    fn parse_objects() {
        valid("{}", json!({}), false);
        valid(r#"{ "foo": true }"#, json!({ "foo": true }), false);
        valid(
            r#"{ "bar": 8, "xoo": "foo" }"#,
            json!({ "bar": 8, "xoo": "foo" }),
            false,
        );
        valid(
            r#"{ "hello": [], "world": {} }"#,
            json!({ "hello": [], "world": {} }),
            false,
        );
        valid(
            r#"{ "a": false, "b": true, "c": [ 7.4 ] }"#,
            json!({ "a": false, "b": true, "c": [7.4] }),
            false,
        );
        valid(
            r#"{ "lineComment": "//", "blockComment": ["/*", "*/"], "brackets": [ ["{", "}"], ["[", "]"], ["(", ")"] ] }"#,
            json!({ "lineComment": "//", "blockComment": ["/*", "*/"], "brackets": [["{", "}"], ["[", "]"], ["(", ")"]] }),
            false,
        );
        valid(
            r#"{ "hello": { "again": { "inside": 5 }, "world": 1 }}"#,
            json!({ "hello": { "again": { "inside": 5 }, "world": 1 } }),
            false,
        );
        valid(r#"{ "foo": /*hello*/true }"#, json!({ "foo": true }), false);
        valid(r#"{ "": true }"#, json!({ "": true }), false);
    }

    #[test]
    fn parse_arrays() {
        valid("[]", json!([]), false);
        valid("[ [],  [ [] ]]", json!([[], [[]]]), false);
        valid("[ 1, 2, 3 ]", json!([1, 2, 3]), false);
        valid(r#"[ { "a": null } ]"#, json!([{ "a": null }]), false);
    }

    #[test]
    fn parse_objects_with_errors() {
        invalid("{,}", Some(json!({})), false);
        invalid(r#"{ "foo": true, }"#, Some(json!({ "foo": true })), false);
        invalid(
            r#"{ "bar": 8 "xoo": "foo" }"#,
            Some(json!({ "bar": 8, "xoo": "foo" })),
            false,
        );
        invalid(r#"{ ,"bar": 8 }"#, Some(json!({ "bar": 8 })), false);
        invalid(r#"{ ,"bar": 8, "foo" }"#, Some(json!({ "bar": 8 })), false);
        invalid(r#"{ "bar": 8, "foo": }"#, Some(json!({ "bar": 8 })), false);
        invalid(r#"{ 8, "foo": 9 }"#, Some(json!({ "foo": 9 })), false);
    }

    #[test]
    fn parse_array_with_errors() {
        invalid("[,]", Some(json!([])), false);
        invalid("[ 1 2, 3 ]", Some(json!([1, 2, 3])), false);
        invalid("[ ,1, 2, 3 ]", Some(json!([1, 2, 3])), false);
        invalid("[ ,1, 2, 3, ]", Some(json!([1, 2, 3])), false);
    }

    #[test]
    fn parse_errors() {
        invalid("", None, false);
        invalid("1,1", Some(json!(1)), false);
    }

    #[test]
    fn parse_trailing_comma() {
        valid(r#"{ "hello": [], }"#, json!({ "hello": [] }), true);
        valid(r#"{ "hello": [] }"#, json!({ "hello": [] }), true);
        valid(
            r#"{ "hello": [], "world": {}, }"#,
            json!({ "hello": [], "world": {} }),
            true,
        );
        valid(
            r#"{ "hello": [], "world": {} }"#,
            json!({ "hello": [], "world": {} }),
            true,
        );
        valid("[ 1, 2, ]", json!([1, 2]), true);
        valid("[ 1, 2 ]", json!([1, 2]), true);

        invalid(r#"{ "hello": [], }"#, Some(json!({ "hello": [] })), false);
        invalid(
            r#"{ "hello": [], "world": {}, }"#,
            Some(json!({ "hello": [], "world": {} })),
            false,
        );
        invalid("[ 1, 2, ]", Some(json!([1, 2])), false);
    }

    #[test]
    fn two_commas_between_two_members_keep_both_and_the_error_is_at_the_second() {
        let text = r#"{ "a": 1,, "b": 2 }"#;
        let recovered = parse(text, true);
        assert_eq!(recovered.value, Some(json!({ "a": 1, "b": 2 })));
        assert_eq!(recovered.first_error, Some(text.find(",,").unwrap() + 1));
    }

    #[test]
    fn a_later_value_of_a_key_replaces_the_earlier_in_its_place() {
        let recovered = parse(r#"{ "a": 1, "b": 2, "a": 3 }"#, true);
        let keys: Vec<(String, Value)> = match recovered.value {
            Some(Value::Object(object)) => object.into_iter().collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            keys,
            [("a".to_owned(), json!(3)), ("b".to_owned(), json!(2))]
        );
    }

    #[test]
    fn an_unclosed_string_comment_or_container_keeps_what_comes_before() {
        invalid(r#"{ "a": [1, 2"#, Some(json!({ "a": [1, 2] })), true);
        invalid(r#"{ "a": "open"#, Some(json!({ "a": "open" })), true);
        invalid(r#"{ "a": 1 /* open"#, Some(json!({ "a": 1 })), true);
        invalid(r#"{ "a": tru, "b": 2 }"#, Some(json!({ "b": 2 })), true);
    }
}
