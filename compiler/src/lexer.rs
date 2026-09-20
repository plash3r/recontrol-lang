#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub length: usize,
}

impl Span {
    fn new(line: usize, column: usize, length: usize) -> Self {
        Self { line, column, length }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier, Number, String,
    Let, Mut, Fn, Struct, Impl, If, Else, For, While, Do, True, False, Return,
    Plus, Minus, Star, Slash, Percent, Equal, EqualEqual,
    NotEqual, Less, LessEqual, Greater, GreaterEqual,
    AndAnd, OrOr, Ampersand, Bang, PlusPlus, MinusMinus,
    LeftParen, RightParen, LeftBrace, RightBrace,
    LeftBracket, RightBracket, Colon, Comma, Dot, Semicolon,
    Newline, Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

pub struct Lexer<'a> {
    chars: Vec<char>,
    current: usize,
    line: usize,
    column: usize,
    _source: &'a str,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { chars: source.chars().collect(), current: 0, line: 1, column: 1, _source: source }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, Vec<LexError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        while !self.is_at_end() {
            let line = self.line;
            let column = self.column;

            match self.advance() {
                ' ' | '\t' | '\r' => {}
                '\n' => tokens.push(self.token(TokenKind::Newline, "\n", line, column, 1)),
                '/' if self.match_char('/') => self.skip_comment(),
                '"' => match self.string(line, column) {
                    Ok(token) => tokens.push(token),
                    Err(error) => errors.push(error),
                },
                c if c.is_ascii_digit() => tokens.push(self.number(c, line, column)),
                c if is_ident_start(c) => tokens.push(self.identifier(c, line, column)),
                '+' if self.match_char('+') => tokens.push(self.token(TokenKind::PlusPlus, "++", line, column, 2)),
                '-' if self.match_char('-') => tokens.push(self.token(TokenKind::MinusMinus, "--", line, column, 2)),
                '=' if self.match_char('=') => tokens.push(self.token(TokenKind::EqualEqual, "==", line, column, 2)),
                '!' if self.match_char('=') => tokens.push(self.token(TokenKind::NotEqual, "!=", line, column, 2)),
                '<' if self.match_char('=') => tokens.push(self.token(TokenKind::LessEqual, "<=", line, column, 2)),
                '>' if self.match_char('=') => tokens.push(self.token(TokenKind::GreaterEqual, ">=", line, column, 2)),
                '&' if self.match_char('&') => tokens.push(self.token(TokenKind::AndAnd, "&&", line, column, 2)),
                '&' => tokens.push(self.token(TokenKind::Ampersand, "&", line, column, 1)),
                '|' if self.match_char('|') => tokens.push(self.token(TokenKind::OrOr, "||", line, column, 2)),
                '+' => tokens.push(self.token(TokenKind::Plus, "+", line, column, 1)),
                '-' => tokens.push(self.token(TokenKind::Minus, "-", line, column, 1)),
                '*' => tokens.push(self.token(TokenKind::Star, "*", line, column, 1)),
                '/' => tokens.push(self.token(TokenKind::Slash, "/", line, column, 1)),
                '%' => tokens.push(self.token(TokenKind::Percent, "%", line, column, 1)),
                '=' => tokens.push(self.token(TokenKind::Equal, "=", line, column, 1)),
                '<' => tokens.push(self.token(TokenKind::Less, "<", line, column, 1)),
                '>' => tokens.push(self.token(TokenKind::Greater, ">", line, column, 1)),
                '!' => tokens.push(self.token(TokenKind::Bang, "!", line, column, 1)),
                '(' => tokens.push(self.token(TokenKind::LeftParen, "(", line, column, 1)),
                ')' => tokens.push(self.token(TokenKind::RightParen, ")", line, column, 1)),
                '{' => tokens.push(self.token(TokenKind::LeftBrace, "{", line, column, 1)),
                '}' => tokens.push(self.token(TokenKind::RightBrace, "}", line, column, 1)),
                '[' => tokens.push(self.token(TokenKind::LeftBracket, "[", line, column, 1)),
                ']' => tokens.push(self.token(TokenKind::RightBracket, "]", line, column, 1)),
                ':' => tokens.push(self.token(TokenKind::Colon, ":", line, column, 1)),
                ',' => tokens.push(self.token(TokenKind::Comma, ",", line, column, 1)),
                '.' => tokens.push(self.token(TokenKind::Dot, ".", line, column, 1)),
                ';' => tokens.push(self.token(TokenKind::Semicolon, ";", line, column, 1)),
                c => errors.push(LexError { message: format!("unexpected character: {}", c), span: Span::new(line, column, 1) }),
            }
        }

        tokens.push(Token { kind: TokenKind::Eof, lexeme: String::new(), span: Span::new(self.line, self.column, 0) });
        if errors.is_empty() { Ok(tokens) } else { Err(errors) }
    }

    fn identifier(&mut self, first: char, line: usize, column: usize) -> Token {
        let mut text = first.to_string();
        while !self.is_at_end() && is_ident_continue(self.peek()) { text.push(self.advance()); }
        let kind = match text.as_str() {
            "let" => TokenKind::Let, "mut" => TokenKind::Mut, "fn" => TokenKind::Fn, "struct" => TokenKind::Struct,
            "impl" => TokenKind::Impl, "if" => TokenKind::If, "else" => TokenKind::Else, "for" => TokenKind::For,
            "while" => TokenKind::While, "do" => TokenKind::Do, "true" => TokenKind::True,
            "false" => TokenKind::False, "return" => TokenKind::Return, _ => TokenKind::Identifier,
        };
        Token { kind, lexeme: text.clone(), span: Span::new(line, column, text.chars().count()) }
    }

    fn number(&mut self, first: char, line: usize, column: usize) -> Token {
        let mut text = first.to_string();
        while !self.is_at_end() && self.peek().is_ascii_digit() { text.push(self.advance()); }
        if !self.is_at_end() && self.peek() == '.' {
            text.push(self.advance());
            while !self.is_at_end() && self.peek().is_ascii_digit() { text.push(self.advance()); }
        }
        while !self.is_at_end() && self.peek().is_ascii_alphanumeric() { text.push(self.advance()); }
        Token { kind: TokenKind::Number, lexeme: text.clone(), span: Span::new(line, column, text.chars().count()) }
    }

    fn string(&mut self, line: usize, column: usize) -> Result<Token, LexError> {
        let mut text = String::new();
        while !self.is_at_end() && self.peek() != '"' {
            if self.peek() == '\n' {
                return Err(LexError { message: "unterminated string literal".into(), span: Span::new(line, column, self.column.saturating_sub(column)) });
            }
            text.push(self.advance());
        }
        if self.is_at_end() {
            return Err(LexError { message: "unterminated string literal".into(), span: Span::new(line, column, self.column.saturating_sub(column)) });
        }
        self.advance();
        Ok(Token { kind: TokenKind::String, lexeme: text.clone(), span: Span::new(line, column, text.chars().count() + 2) })
    }

    fn skip_comment(&mut self) { while !self.is_at_end() && self.peek() != '\n' { self.advance(); } }
    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.peek() != expected { return false; }
        self.advance(); true
    }
    fn peek(&self) -> char { self.chars.get(self.current).copied().unwrap_or('\0') }
    fn advance(&mut self) -> char {
        let c = self.chars[self.current];
        self.current += 1;
        if c == '\n' { self.line += 1; self.column = 1; } else { self.column += 1; }
        c
    }
    fn is_at_end(&self) -> bool { self.current >= self.chars.len() }
    fn token(&self, kind: TokenKind, lexeme: &str, line: usize, column: usize, length: usize) -> Token {
        Token { kind, lexeme: lexeme.into(), span: Span::new(line, column, length) }
    }
}

fn is_ident_start(c: char) -> bool { c == '_' || c.is_ascii_alphabetic() }
fn is_ident_continue(c: char) -> bool { c == '_' || c.is_ascii_alphanumeric() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_basic_program() {
        let source = "fn main() {\n    let value = 42u64\n    println(\"Hello\")\n}";
        let tokens = Lexer::new(source).tokenize().unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].lexeme, "main");
        assert!(tokens.iter().any(|t| t.lexeme == "42u64"));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::String));
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
    }

    #[test]
    fn reports_invalid_character() {
        let errors = Lexer::new("let x = @").tokenize().unwrap_err();
        assert_eq!(errors[0].message, "unexpected character: @");
        assert_eq!(errors[0].span.line, 1);
    }

    #[test]
    fn reports_unterminated_string() {
        let errors = Lexer::new("let x = \"hello").tokenize().unwrap_err();
        assert_eq!(errors[0].message, "unterminated string literal");
    }
}
