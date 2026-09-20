#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier, Number, String,
    Let, Fn, Struct, If, Else, For, While, Do, True, False, Return,
    Plus, Minus, Star, Slash, Percent, Equal, EqualEqual,
    NotEqual, Less, LessEqual, Greater, GreaterEqual,
    AndAnd, OrOr, Bang, PlusPlus, MinusMinus,
    LeftParen, RightParen, LeftBrace, RightBrace,
    LeftBracket, RightBracket, Colon, Comma, Dot, Semicolon,
    Newline, Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
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

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        while !self.is_at_end() {
            let line = self.line;
            let column = self.column;
            match self.advance() {
                ' ' | '\t' | '\r' => {}
                '\n' => tokens.push(self.token(TokenKind::Newline, "\n", line, column)),
                '/' if self.match_char('/') => self.skip_comment(),
                '"' => tokens.push(self.string(line, column)),
                c if c.is_ascii_digit() => tokens.push(self.number(c, line, column)),
                c if is_ident_start(c) => tokens.push(self.identifier(c, line, column)),
                '+' if self.match_char('+') => tokens.push(self.token(TokenKind::PlusPlus, "++", line, column)),
                '-' if self.match_char('-') => tokens.push(self.token(TokenKind::MinusMinus, "--", line, column)),
                '=' if self.match_char('=') => tokens.push(self.token(TokenKind::EqualEqual, "==", line, column)),
                '!' if self.match_char('=') => tokens.push(self.token(TokenKind::NotEqual, "!=", line, column)),
                '<' if self.match_char('=') => tokens.push(self.token(TokenKind::LessEqual, "<=", line, column)),
                '>' if self.match_char('=') => tokens.push(self.token(TokenKind::GreaterEqual, ">=", line, column)),
                '&' if self.match_char('&') => tokens.push(self.token(TokenKind::AndAnd, "&&", line, column)),
                '|' if self.match_char('|') => tokens.push(self.token(TokenKind::OrOr, "||", line, column)),
                '+' => tokens.push(self.token(TokenKind::Plus, "+", line, column)),
                '-' => tokens.push(self.token(TokenKind::Minus, "-", line, column)),
                '*' => tokens.push(self.token(TokenKind::Star, "*", line, column)),
                '/' => tokens.push(self.token(TokenKind::Slash, "/", line, column)),
                '%' => tokens.push(self.token(TokenKind::Percent, "%", line, column)),
                '=' => tokens.push(self.token(TokenKind::Equal, "=", line, column)),
                '<' => tokens.push(self.token(TokenKind::Less, "<", line, column)),
                '>' => tokens.push(self.token(TokenKind::Greater, ">", line, column)),
                '!' => tokens.push(self.token(TokenKind::Bang, "!", line, column)),
                '(' => tokens.push(self.token(TokenKind::LeftParen, "(", line, column)),
                ')' => tokens.push(self.token(TokenKind::RightParen, ")", line, column)),
                '{' => tokens.push(self.token(TokenKind::LeftBrace, "{", line, column)),
                '}' => tokens.push(self.token(TokenKind::RightBrace, "}", line, column)),
                '[' => tokens.push(self.token(TokenKind::LeftBracket, "[", line, column)),
                ']' => tokens.push(self.token(TokenKind::RightBracket, "]", line, column)),
                ':' => tokens.push(self.token(TokenKind::Colon, ":", line, column)),
                ',' => tokens.push(self.token(TokenKind::Comma, ",", line, column)),
                '.' => tokens.push(self.token(TokenKind::Dot, ".", line, column)),
                ';' => tokens.push(self.token(TokenKind::Semicolon, ";", line, column)),
                _ => {}
            }
        }

        tokens.push(Token { kind: TokenKind::Eof, lexeme: String::new(), line: self.line, column: self.column });
        tokens
    }

    fn identifier(&mut self, first: char, line: usize, column: usize) -> Token {
        let mut text = first.to_string();
        while !self.is_at_end() && is_ident_continue(self.peek()) { text.push(self.advance()); }
        let kind = match text.as_str() {
            "let" => TokenKind::Let, "fn" => TokenKind::Fn, "struct" => TokenKind::Struct,
            "if" => TokenKind::If, "else" => TokenKind::Else, "for" => TokenKind::For,
            "while" => TokenKind::While, "do" => TokenKind::Do, "true" => TokenKind::True,
            "false" => TokenKind::False, "return" => TokenKind::Return,
            _ => TokenKind::Identifier,
        };
        Token { kind, lexeme: text, line, column }
    }

    fn number(&mut self, first: char, line: usize, column: usize) -> Token {
        let mut text = first.to_string();
        while !self.is_at_end() && self.peek().is_ascii_digit() { text.push(self.advance()); }
        if !self.is_at_end() && self.peek() == '.' {
            text.push(self.advance());
            while !self.is_at_end() && self.peek().is_ascii_digit() { text.push(self.advance()); }
        }
        while !self.is_at_end() && self.peek().is_ascii_alphanumeric() { text.push(self.advance()); }
        Token { kind: TokenKind::Number, lexeme: text, line, column }
    }

    fn string(&mut self, line: usize, column: usize) -> Token {
        let mut text = String::new();
        while !self.is_at_end() && self.peek() != '"' {
            let c = self.advance();
            if c == '\n' { self.line += 1; self.column = 1; }
            text.push(c);
        }
        if !self.is_at_end() { self.advance(); }
        Token { kind: TokenKind::String, lexeme: text, line, column }
    }

    fn skip_comment(&mut self) { while !self.is_at_end() && self.peek() != '\n' { self.advance(); } }
    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.peek() != expected { return false; }
        self.advance(); true
    }
    fn peek(&self) -> char { self.chars.get(self.current).copied().unwrap_or('\0') }
    fn advance(&mut self) -> char { let c = self.chars[self.current]; self.current += 1; self.column += 1; c }
    fn is_at_end(&self) -> bool { self.current >= self.chars.len() }
    fn token(&self, kind: TokenKind, lexeme: &str, line: usize, column: usize) -> Token {
        Token { kind, lexeme: lexeme.into(), line, column }
    }
}

fn is_ident_start(c: char) -> bool { c == '_' || c.is_ascii_alphabetic() }
fn is_ident_continue(c: char) -> bool { c == '_' || c.is_ascii_alphanumeric() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_basic_program() {
        let tokens = Lexer::new("fn main() { let value = 42u64\n println(\"Hello\") }").tokenize();
        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].lexeme, "main");
        assert!(tokens.iter().any(|t| t.lexeme == "42u64"));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::String));
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
    }
}
