//! The Gleam tokenizer.

use crate::ast::{IntValue, ParseError, Span};

/// A single lexed token with its position in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// 1-based line of the token start.
    pub line: u32,
    /// 1-based column of the token start.
    pub col: u32,
}

impl Token {
    fn new(kind: TokenKind, span: Span, line: u32, col: u32) -> Self {
        Self {
            kind,
            span,
            line,
            col,
        }
    }
}

/// The kinds of tokens in Gleam source code.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // -- Literals ----------------------------------------------------------
    Int(String),
    Float(f64, String),
    String(String),

    // -- Names --------------------------------------------------------------
    /// A lowercase identifier: a variable or function name.
    Variable(String),
    /// A discard identifier: `_` or `_name`.
    DiscardVariable(String),
    /// An uppercase identifier: a type or constructor name.
    TypeName(String),

    // -- Keywords -----------------------------------------------------------
    Fn,
    Let,
    Assert,
    Case,
    If,
    As,
    Type,
    Const,
    Pub,
    Opaque,
    Import,
    Echo,
    Todo,
    Panic,
    Use,
    True,
    False,
    Nil,

    // -- Attributes ---------------------------------------------------------
    /// `@external`
    External,
    /// `@deprecated`
    Deprecated,

    // -- Operators ----------------------------------------------------------
    Plus,
    Minus,
    Times,
    Slash,
    Percent,
    PlusDot,
    MinusDot,
    TimesDot,
    SlashDot,
    EqualEqual,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    GreaterThanDot,
    LessThanDot,
    GreaterThanOrEqualDot,
    LessThanOrEqualDot,
    And,
    Or,
    Bang,
    Concatenate,
    PipeTo,
    Arrow,
    /// The `<-` of `use` expressions.
    DashArrow,
    Assign,
    Pipe,

    // -- Punctuation --------------------------------------------------------
    OpenBlock,
    CloseBlock,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    /// `#(` — the start of a tuple.
    HashOpen,
    /// `<<` — the start of a bit array.
    BitArrayStart,
    /// `>>` — the end of a bit array.
    BitArrayEnd,
    Comma,
    Dot,
    TwoDots,
    Colon,

    // -- Comments -----------------------------------------------------------
    /// A documentation comment: `/// ...`.
    DocComment(String),
    /// A module documentation comment: `//// ...`.
    ModuleDocComment(String),

    EndOfFile,
}

impl TokenKind {
    /// A short human readable description, used in error messages.
    pub fn describe(&self) -> String {
        use TokenKind::*;
        match self {
            Int(_) => "an integer literal".into(),
            Float(_, _) => "a float literal".into(),
            String(_) => "a string literal".into(),
            Variable(name) => format!("the variable `{name}`"),
            DiscardVariable(name) => format!("the discard `{name}`"),
            TypeName(name) => format!("the type name `{name}`"),
            Fn => "`fn`".into(),
            Let => "`let`".into(),
            Assert => "`assert`".into(),
            Case => "`case`".into(),
            If => "`if`".into(),
            As => "`as`".into(),
            Type => "`type`".into(),
            Const => "`const`".into(),
            Pub => "`pub`".into(),
            Opaque => "`opaque`".into(),
            Import => "`import`".into(),
            Echo => "`echo`".into(),
            Todo => "`todo`".into(),
            Panic => "`panic`".into(),
            Use => "`use`".into(),
            True => "`True`".into(),
            False => "`False`".into(),
            Nil => "`Nil`".into(),
            External => "`@external`".into(),
            Deprecated => "`@deprecated`".into(),
            Plus => "`+`".into(),
            Minus => "`-`".into(),
            Times => "`*`".into(),
            Slash => "`/`".into(),
            Percent => "`%`".into(),
            PlusDot => "`+. `".into(),
            MinusDot => "`-.`".into(),
            TimesDot => "`*.`".into(),
            SlashDot => "`/.`".into(),
            EqualEqual => "`==`".into(),
            NotEqual => "`!=`".into(),
            GreaterThan => "`>`".into(),
            LessThan => "`<`".into(),
            GreaterThanOrEqual => "`>=`".into(),
            LessThanOrEqual => "`<=`".into(),
            GreaterThanDot => "`>.`".into(),
            LessThanDot => "`<.`".into(),
            GreaterThanOrEqualDot => "`>=.`".into(),
            LessThanOrEqualDot => "`<=.`".into(),
            And => "`&&`".into(),
            Or => "`||`".into(),
            Bang => "`!`".into(),
            Concatenate => "`<>`".into(),
            PipeTo => "`|>`".into(),
            Arrow => "`->`".into(),
            DashArrow => "`<-`".into(),
            Assign => "`=`".into(),
            Pipe => "`|`".into(),
            OpenBlock => "`{`".into(),
            CloseBlock => "`}`".into(),
            OpenSquare => "`[`".into(),
            CloseSquare => "`]`".into(),
            OpenParen => "`(`".into(),
            CloseParen => "`)`".into(),
            HashOpen => "`#(`".into(),
            BitArrayStart => "`<<`".into(),
            BitArrayEnd => "`>>`".into(),
            Comma => "`,`".into(),
            Dot => "`.`".into(),
            TwoDots => "`..`".into(),
            Colon => "':'".into(),
            DocComment(_) => "a documentation comment".into(),
            ModuleDocComment(_) => "a module documentation comment".into(),
            EndOfFile => "end of file".into(),
        }
    }
}

/// Tokenize a Gleam source string.
pub fn lex(source: &str) -> Result<Vec<Token>, ParseError> {
    Lexer::new(source).lex()
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    /// Position info captured at the start of the current token.
    token_start: usize,
    token_line: u32,
    token_col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
            col: 1,
            token_start: 0,
            token_line: 1,
            token_col: 1,
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia(&mut tokens)?;
            if self.pos >= self.src.len() {
                tokens.push(Token::new(
                    TokenKind::EndOfFile,
                    Span::point(self.pos),
                    self.line,
                    self.col,
                ));
                return Ok(tokens);
            }
            let (kind, end) = self.lex_one()?;
            tokens.push(Token::new(
                kind,
                Span::new(self.token_start, end),
                self.token_line,
                self.token_col,
            ));
        }
    }

    fn skip_trivia(&mut self, tokens: &mut Vec<Token>) -> Result<(), ParseError> {
        loop {
            let rest = &self.src[self.pos..];
            match rest.chars().next() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.advance(1);
                }
                Some('/') => {
                    let start = self.pos;
                    let (line, col) = (self.line, self.col);
                    if rest.starts_with("////") {
                        // Module documentation comment.
                        let end = self.line_end();
                        let text = self.src[start + 4..end]
                            .trim_start_matches(' ')
                            .trim_end_matches('\n')
                            .to_string();
                        self.skip_to_line_end();
                        tokens.push(Token::new(
                            TokenKind::ModuleDocComment(text),
                            Span::new(start, end),
                            line,
                            col,
                        ));
                    } else if rest.starts_with("///") {
                        // Documentation comment.
                        let end = self.line_end();
                        let text = self.src[start + 3..end]
                            .trim_start_matches(' ')
                            .trim_end_matches('\n')
                            .to_string();
                        self.skip_to_line_end();
                        tokens.push(Token::new(
                            TokenKind::DocComment(text),
                            Span::new(start, end),
                            line,
                            col,
                        ));
                    } else if rest.starts_with("//") {
                        // Regular comment: skip to end of line.
                        self.skip_to_line_end();
                    } else {
                        // A lone `/` is the division operator, not a comment.
                        return Ok(());
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// The byte index just past the end of the current line (or EOF).
    fn line_end(&self) -> usize {
        self.src[self.pos..]
            .find('\n')
            .map(|i| self.pos + i + 1)
            .unwrap_or(self.src.len())
    }

    /// Skip to the end of the current line, updating line/col tracking.
    fn skip_to_line_end(&mut self) {
        let end = self.line_end();
        while self.pos < end {
            let ch = self.src[self.pos..].chars().next().unwrap();
            self.pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    fn advance(&mut self, chars: usize) {
        for _ in 0..chars {
            if self.pos >= self.src.len() {
                break;
            }
            let ch = self.src[self.pos..].chars().next().unwrap();
            self.pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(message, Span::point(self.pos), self.line, self.col)
    }

    /// Lex a single token. Returns the kind and the end byte offset.
    fn lex_one(&mut self) -> Result<(TokenKind, usize), ParseError> {
        self.token_start = self.pos;
        self.token_line = self.line;
        self.token_col = self.col;

        let rest = &self.src[self.pos..];
        let ch = rest.chars().next().unwrap();

        match ch {
            '0'..='9' => self.lex_number(),
            '"' => self.lex_string(),
            'a'..='z' | 'A'..='Z' | '_' => self.lex_identifier(),
            '@' => self.lex_attribute(),
            _ => self.lex_operator(ch),
        }
    }

    fn lex_number(&mut self) -> Result<(TokenKind, usize), ParseError> {
        let rest = &self.src[self.pos..];
        let start = self.pos;

        // Radix prefixes: 0b, 0o, 0x
        if let Some(radix_prefix) = ["0b", "0B", "0o", "0O", "0x", "0X"]
            .iter()
            .find(|p| rest.starts_with(**p))
        {
            self.advance(radix_prefix.len());
            let digit_start = self.pos;
            let mut has_digit = false;
            while let Some(c) = self.src[self.pos..].chars().next() {
                let ok = match radix_prefix.to_ascii_lowercase().as_str() {
                    "0x" => c.is_ascii_hexdigit(),
                    "0o" => ('0'..='7').contains(&c),
                    _ => ('0'..='1').contains(&c),
                };
                if ok {
                    has_digit = true;
                    self.advance(1);
                } else if c == '_' && has_digit {
                    self.advance(1);
                } else {
                    break;
                }
            }
            let text = &self.src[digit_start..self.pos];
            if !has_digit || text.ends_with('_') {
                return Err(self.error("invalid number literal"));
            }
            let full = self.src[start..self.pos].to_string();
            if (IntValue { raw: full.clone() }).as_u64().is_none() {
                return Err(self.error("integer literal is too large"));
            }
            return Ok((TokenKind::Int(full), self.pos));
        }

        // Decimal integer, possibly becoming a float.
        while let Some(c) = self.src[self.pos..].chars().next() {
            if c.is_ascii_digit() || c == '_' {
                self.advance(1);
            } else {
                break;
            }
        }
        let int_text = &self.src[start..self.pos];
        if int_text.ends_with('_') {
            return Err(self.error("invalid number literal"));
        }

        // Float?  `.` followed by a digit.
        let after = &self.src[self.pos..];
        if after.starts_with('.') && after[1..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
            self.advance(1); // the dot
            let frac_start = self.pos;
            while let Some(c) = self.src[self.pos..].chars().next() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance(1);
                } else {
                    break;
                }
            }
            let frac_text = &self.src[frac_start..self.pos];
            if frac_text.is_empty() || frac_text.ends_with('_') {
                return Err(self.error("invalid float literal"));
            }
            // Optional exponent: e/E [+-] digits
            let after2 = &self.src[self.pos..];
            if matches!(after2.chars().next(), Some('e' | 'E')) {
                self.advance(1);
                if matches!(self.src[self.pos..].chars().next(), Some('+' | '-')) {
                    self.advance(1);
                }
                while let Some(c) = self.src[self.pos..].chars().next() {
                    if c.is_ascii_digit() || c == '_' {
                        self.advance(1);
                    } else {
                        break;
                    }
                }
            }
            let full = self.src[start..self.pos].to_string();
            let cleaned: String = full.chars().filter(|c| *c != '_').collect();
            match cleaned.parse::<f64>() {
                Ok(v) => return Ok((TokenKind::Float(v, full), self.pos)),
                Err(_) => return Err(self.error("invalid float literal")),
            }
        }

        let full = int_text.to_string();
        let cleaned: String = full.chars().filter(|c| *c != '_').collect();
        if cleaned.parse::<u64>().is_err() {
            return Err(self.error("integer literal is too large"));
        }
        Ok((TokenKind::Int(full), self.pos))
    }

    fn lex_string(&mut self) -> Result<(TokenKind, usize), ParseError> {
        let start = self.pos;
        self.advance(1); // opening quote
        let mut value = String::new();
        loop {
            let rest = &self.src[self.pos..];
            match rest.chars().next() {
                None => {
                    return Err(ParseError::new(
                        "unterminated string literal",
                        Span::new(start, self.pos),
                        self.token_line,
                        self.token_col,
                    ))
                }
                Some('"') => {
                    self.advance(1);
                    break;
                }
                Some('\\') => {
                    self.advance(1);
                    let rest2 = &self.src[self.pos..];
                    match rest2.chars().next() {
                        Some('"') => {
                            value.push('"');
                            self.advance(1);
                        }
                        Some('\\') => {
                            value.push('\\');
                            self.advance(1);
                        }
                        Some('f') => {
                            value.push('\u{000C}');
                            self.advance(1);
                        }
                        Some('n') => {
                            value.push('\n');
                            self.advance(1);
                        }
                        Some('r') => {
                            value.push('\r');
                            self.advance(1);
                        }
                        Some('t') => {
                            value.push('\t');
                            self.advance(1);
                        }
                        Some('u') => {
                            // \u{hex}
                            self.advance(1);
                            let rest3 = &self.src[self.pos..];
                            if !rest3.starts_with('{') {
                                return Err(self.error("expected `{` in unicode escape"));
                            }
                            self.advance(1);
                            let hex_start = self.pos;
                            while let Some(c) = self.src[self.pos..].chars().next() {
                                if c.is_ascii_hexdigit() && self.pos - hex_start < 12 {
                                    self.advance(1);
                                } else {
                                    break;
                                }
                            }
                            let rest4 = &self.src[self.pos..];
                            if !rest4.starts_with('}') {
                                return Err(self.error("expected `}` in unicode escape"));
                            }
                            let hex = self.src[hex_start..self.pos].to_string();
                            self.advance(1);
                            let cp = match u32::from_str_radix(&hex, 16) {
                                Ok(cp) => cp,
                                Err(_) => return Err(self.error("invalid unicode codepoint")),
                            };
                            match char::from_u32(cp) {
                                Some(c) => value.push(c),
                                None => return Err(self.error("invalid unicode codepoint")),
                            }
                        }
                        Some(other) => {
                            return Err(self.error(format!(
                                "unknown escape sequence `\\{other}`"
                            )))
                        }
                        None => {
                            return Err(self.error("unterminated string literal"))
                        }
                    }
                }
                Some(_) => {
                    let c = rest.chars().next().unwrap();
                    value.push(c);
                    self.advance(1);
                }
            }
        }
        Ok((TokenKind::String(value), self.pos))
    }

    fn lex_identifier(&mut self) -> Result<(TokenKind, usize), ParseError> {
        let start = self.pos;
        while let Some(c) = self.src[self.pos..].chars().next() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.advance(1);
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = match text {
            // Keywords (case sensitive).
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "assert" => TokenKind::Assert,
            "case" => TokenKind::Case,
            "if" => TokenKind::If,
            "as" => TokenKind::As,
            "type" => TokenKind::Type,
            "const" => TokenKind::Const,
            "pub" => TokenKind::Pub,
            "opaque" => TokenKind::Opaque,
            "import" => TokenKind::Import,
            "echo" => TokenKind::Echo,
            "todo" => TokenKind::Todo,
            "panic" => TokenKind::Panic,
            "use" => TokenKind::Use,
            "True" => TokenKind::True,
            "False" => TokenKind::False,
            "Nil" => TokenKind::Nil,
            _ if text.starts_with('_') => TokenKind::DiscardVariable(text.to_string()),
            _ if text.starts_with(|c: char| c.is_ascii_uppercase()) => {
                TokenKind::TypeName(text.to_string())
            }
            _ => TokenKind::Variable(text.to_string()),
        };
        Ok((kind, self.pos))
    }

    fn lex_attribute(&mut self) -> Result<(TokenKind, usize), ParseError> {
        let start = self.pos;
        self.advance(1); // @
        while let Some(c) = self.src[self.pos..].chars().next() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.advance(1);
            } else {
                break;
            }
        }
        let text = &self.src[start + 1..self.pos];
        match text {
            "external" => Ok((TokenKind::External, self.pos)),
            "deprecated" => Ok((TokenKind::Deprecated, self.pos)),
            _ => Err(self.error(format!(
                "unknown attribute `@{text}` (expected `@external` or `@deprecated`)"
            ))),
        }
    }

    fn lex_operator(&mut self, ch: char) -> Result<(TokenKind, usize), ParseError> {
        let rest = &self.src[self.pos..];
        let kind = if ch == '#' {
            if rest.starts_with("#(") {
                self.advance(2);
                TokenKind::HashOpen
            } else {
                return Err(self.error("expected `#(` to start a tuple"));
            }
        } else if ch == '<' {
            if rest.starts_with("<<") {
                self.advance(2);
                TokenKind::BitArrayStart
            } else if rest.starts_with("<-") {
                self.advance(2);
                TokenKind::DashArrow
            } else if rest.starts_with("<>") {
                self.advance(2);
                TokenKind::Concatenate
            } else if rest.starts_with("<=.") {
                self.advance(3);
                TokenKind::LessThanOrEqualDot
            } else if rest.starts_with("<=") {
                self.advance(2);
                TokenKind::LessThanOrEqual
            } else if rest.starts_with("<.") {
                self.advance(2);
                TokenKind::LessThanDot
            } else {
                self.advance(1);
                TokenKind::LessThan
            }
        } else if ch == '>' {
            if rest.starts_with(">>") {
                self.advance(2);
                TokenKind::BitArrayEnd
            } else if rest.starts_with(">=.") {
                self.advance(3);
                TokenKind::GreaterThanOrEqualDot
            } else if rest.starts_with(">=") {
                self.advance(2);
                TokenKind::GreaterThanOrEqual
            } else if rest.starts_with(">.") {
                self.advance(2);
                TokenKind::GreaterThanDot
            } else {
                self.advance(1);
                TokenKind::GreaterThan
            }
        } else if ch == '|' {
            if rest.starts_with("|>") {
                self.advance(2);
                TokenKind::PipeTo
            } else if rest.starts_with("||") {
                self.advance(2);
                TokenKind::Or
            } else {
                self.advance(1);
                TokenKind::Pipe
            }
        } else if ch == '=' {
            if rest.starts_with("==") {
                self.advance(2);
                TokenKind::EqualEqual
            } else {
                self.advance(1);
                TokenKind::Assign
            }
        } else if ch == '!' {
            if rest.starts_with("!=") {
                self.advance(2);
                TokenKind::NotEqual
            } else {
                self.advance(1);
                TokenKind::Bang
            }
        } else if ch == '&' {
            if rest.starts_with("&&") {
                self.advance(2);
                TokenKind::And
            } else {
                return Err(self.error("expected `&&`"));
            }
        } else if ch == '+' {
            if rest.starts_with("+.") {
                self.advance(2);
                TokenKind::PlusDot
            } else {
                self.advance(1);
                TokenKind::Plus
            }
        } else if ch == '-' {
            if rest.starts_with("->") {
                self.advance(2);
                TokenKind::Arrow
            } else if rest.starts_with("-.") {
                self.advance(2);
                TokenKind::MinusDot
            } else {
                self.advance(1);
                TokenKind::Minus
            }
        } else if ch == '*' {
            if rest.starts_with("*.") {
                self.advance(2);
                TokenKind::TimesDot
            } else {
                self.advance(1);
                TokenKind::Times
            }
        } else if ch == '/' {
            if rest.starts_with("/.") {
                self.advance(2);
                TokenKind::SlashDot
            } else {
                // A single `/` is division; `//` comments are handled in trivia.
                self.advance(1);
                TokenKind::Slash
            }
        } else if ch == '.' {
            if rest.starts_with("..") {
                self.advance(2);
                TokenKind::TwoDots
            } else {
                self.advance(1);
                TokenKind::Dot
            }
        } else {
            match ch {
                '{' => {
                    self.advance(1);
                    TokenKind::OpenBlock
                }
                '}' => {
                    self.advance(1);
                    TokenKind::CloseBlock
                }
                '[' => {
                    self.advance(1);
                    TokenKind::OpenSquare
                }
                ']' => {
                    self.advance(1);
                    TokenKind::CloseSquare
                }
                '(' => {
                    self.advance(1);
                    TokenKind::OpenParen
                }
                ')' => {
                    self.advance(1);
                    TokenKind::CloseParen
                }
                ',' => {
                    self.advance(1);
                    TokenKind::Comma
                }
                ':' => {
                    self.advance(1);
                    TokenKind::Colon
                }
                '%' => {
                    self.advance(1);
                    TokenKind::Percent
                }
                _ => return Err(self.error(format!("unexpected character `{ch}`"))),
            }
        };
        Ok((kind, self.pos))
    }
}
