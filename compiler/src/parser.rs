use crate::ast::*;
use crate::lexer::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, current: 0 } }

    pub fn parse(mut self) -> Result<Program, Vec<ParseError>> {
        let mut items = Vec::new();
        let mut errors = Vec::new();
        self.skip_newlines();

        while !self.check(TokenKind::Eof) {
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(error) => { errors.push(error); self.synchronize(); }
            }
            self.skip_newlines();
        }

        if errors.is_empty() { Ok(Program { items }) } else { Err(errors) }
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek().kind {
            TokenKind::Fn => self.parse_function().map(Item::Function),
            TokenKind::Struct => self.parse_struct().map(Item::Struct),
            _ => Err(self.error_here("expected fn or struct")),
        }
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(TokenKind::Fn, "expected fn")?;
        let name = self.expect_identifier("expected function name")?;
        self.expect(TokenKind::LeftParen, "expected ( after function name")?;
        let mut params = Vec::new();

        if !self.check(TokenKind::RightParen) {
            loop {
                let param_name = self.expect_identifier("expected parameter name")?;
                self.expect(TokenKind::Colon, "expected : after parameter name")?;
                let ty = self.parse_type()?;
                params.push(Parameter { name: param_name, ty });
                if !self.match_kind(TokenKind::Comma) { break; }
                self.skip_newlines();
                if self.check(TokenKind::RightParen) { break; }
            }
        }

        self.expect(TokenKind::RightParen, "expected ) after parameters")?;
        let return_type = if self.match_kind(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else { None };

        self.skip_newlines();
        let body = self.parse_block()?;
        Ok(Function { name, params, return_type, body })
    }

    fn parse_struct(&mut self) -> Result<StructDef, ParseError> {
        self.expect(TokenKind::Struct, "expected struct")?;
        let name = self.expect_identifier("expected struct name")?;
        self.skip_newlines();
        self.expect(TokenKind::LeftBrace, "expected { after struct name")?;
        let mut fields = Vec::new();
        self.skip_newlines();

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            let field_name = self.expect_identifier("expected field name")?;
            self.expect(TokenKind::Colon, "expected : after field name")?;
            let ty = self.parse_type()?;
            fields.push(Field { name: field_name, ty });

            if self.match_kind(TokenKind::Comma) || self.match_kind(TokenKind::Semicolon) {
                self.skip_newlines();
            } else if self.check(TokenKind::Newline) {
                self.skip_newlines();
            } else if !self.check(TokenKind::RightBrace) {
                return Err(self.error_here("expected newline, comma, semicolon, or } after field"));
            }
        }

        self.expect(TokenKind::RightBrace, "expected } after struct fields")?;
        Ok(StructDef { name, fields })
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect(TokenKind::LeftBrace, "expected {")?;
        self.skip_newlines();
        let mut statements = Vec::new();

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            statements.push(self.parse_statement()?);
            self.consume_statement_terminator()?;
            self.skip_newlines();
        }

        self.expect(TokenKind::RightBrace, "expected }")?;
        Ok(Block { statements })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Let => self.parse_let(false),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Return => self.parse_return(),
            TokenKind::LeftBrace => self.parse_block().map(Stmt::Block),
            _ => self.parse_expression().map(Stmt::Expr),
        }
    }

    fn parse_let(&mut self, _in_for: bool) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::Let, "expected let")?;
        let mutable = self.match_kind(TokenKind::Mut);
        let name = self.expect_identifier("expected variable name")?;
        let ty = if self.match_kind(TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        let initializer = if self.match_kind(TokenKind::Equal) { Some(self.parse_expression()?) } else { None };

        if ty.is_none() && initializer.is_none() {
            return Err(self.error_here("variable declaration needs a type or initializer"));
        }
        Ok(Stmt::Let { name, mutable, ty, initializer })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::If, "expected if")?;
        let condition = self.parse_expression()?;
        self.skip_newlines();
        let then_branch = self.parse_block()?;
        self.skip_newlines();

        let else_branch = if self.match_kind(TokenKind::Else) {
            self.skip_newlines();
            Some(self.parse_block()?)
        } else { None };

        Ok(Stmt::If { condition, then_branch, else_branch })
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::While, "expected while")?;
        let condition = self.parse_expression()?;
        self.skip_newlines();
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_do_while(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::Do, "expected do")?;
        self.skip_newlines();
        let body = self.parse_block()?;
        self.skip_newlines();
        self.expect(TokenKind::While, "expected while after do block")?;
        let condition = self.parse_expression()?;
        Ok(Stmt::DoWhile { body, condition })
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::For, "expected for")?;
        self.expect(TokenKind::LeftParen, "expected ( after for")?;
        self.skip_newlines();

        let initializer = if self.check(TokenKind::Semicolon) {
            None
        } else if self.check(TokenKind::Let) {
            Some(Box::new(self.parse_let(true)?))
        } else {
            Some(Box::new(Stmt::Expr(self.parse_expression()?)))
        };

        self.expect(TokenKind::Semicolon, "expected ; after for initializer")?;
        self.skip_newlines();

        let condition = if self.check(TokenKind::Semicolon) { None } else { Some(self.parse_expression()?) };
        self.expect(TokenKind::Semicolon, "expected ; after for condition")?;
        self.skip_newlines();

        let update = if self.check(TokenKind::RightParen) { None } else { Some(self.parse_expression()?) };
        self.skip_newlines();
        self.expect(TokenKind::RightParen, "expected ) after for clauses")?;
        self.skip_newlines();

        let body = self.parse_block()?;
        Ok(Stmt::For { initializer, condition, update, body })
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::Return, "expected return")?;
        if self.check(TokenKind::Newline) || self.check(TokenKind::Semicolon) || self.check(TokenKind::RightBrace) {
            Ok(Stmt::Return(None))
        } else {
            Ok(Stmt::Return(Some(self.parse_expression()?)))
        }
    }

    fn parse_type(&mut self) -> Result<TypeRef, ParseError> {
        Ok(TypeRef { name: self.expect_identifier("expected type name")? })
    }

    fn parse_expression(&mut self) -> Result<Expr, ParseError> { self.parse_assignment() }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_binary(0)?;
        if self.match_kind(TokenKind::Equal) {
            let value = self.parse_assignment()?;
            return Ok(Expr::Assignment { target: Box::new(left), value: Box::new(value) });
        }
        Ok(left)
    }

    fn parse_binary(&mut self, min_precedence: u8) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let (op, precedence) = match self.binary_operator() {
                Some(value) => value,
                None => break,
            };
            if precedence < min_precedence { break; }
            self.advance();
            let right = self.parse_binary(precedence + 1)?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn binary_operator(&self) -> Option<(BinaryOp, u8)> {
        match self.peek().kind {
            TokenKind::OrOr => Some((BinaryOp::Or, 1)),
            TokenKind::AndAnd => Some((BinaryOp::And, 2)),
            TokenKind::EqualEqual => Some((BinaryOp::Equal, 3)),
            TokenKind::NotEqual => Some((BinaryOp::NotEqual, 3)),
            TokenKind::Less => Some((BinaryOp::Less, 4)),
            TokenKind::LessEqual => Some((BinaryOp::LessEqual, 4)),
            TokenKind::Greater => Some((BinaryOp::Greater, 4)),
            TokenKind::GreaterEqual => Some((BinaryOp::GreaterEqual, 4)),
            TokenKind::Plus => Some((BinaryOp::Add, 5)),
            TokenKind::Minus => Some((BinaryOp::Subtract, 5)),
            TokenKind::Star => Some((BinaryOp::Multiply, 6)),
            TokenKind::Slash => Some((BinaryOp::Divide, 6)),
            TokenKind::Percent => Some((BinaryOp::Modulo, 6)),
            _ => None,
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let op = match self.peek().kind {
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Minus => Some(UnaryOp::Minus),
            TokenKind::Plus => Some(UnaryOp::Plus),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            return Ok(Expr::Unary { op, expr: Box::new(self.parse_unary()?) });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek().kind {
                TokenKind::LeftParen => {
                    self.advance();
                    let mut args = Vec::new();
                    self.skip_newlines();

                    if !self.check(TokenKind::RightParen) {
                        loop {
                            args.push(self.parse_expression()?);
                            self.skip_newlines();
                            if !self.match_kind(TokenKind::Comma) { break; }
                            self.skip_newlines();
                        }
                    }

                    self.expect(TokenKind::RightParen, "expected ) after arguments")?;
                    expr = Expr::Call { callee: Box::new(expr), args };
                }
                TokenKind::Dot => {
                    self.advance();
                    let name = self.expect_identifier("expected member name after dot")?;
                    expr = Expr::Member { object: Box::new(expr), name };
                }
                TokenKind::PlusPlus => {
                    self.advance();
                    expr = Expr::Postfix { expr: Box::new(expr), op: PostfixOp::Increment };
                }
                TokenKind::MinusMinus => {
                    self.advance();
                    expr = Expr::Postfix { expr: Box::new(expr), op: PostfixOp::Decrement };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Number => Ok(Expr::Literal(Literal::Number(token.lexeme))),
            TokenKind::String => Ok(Expr::Literal(Literal::String(token.lexeme))),
            TokenKind::True => Ok(Expr::Literal(Literal::Bool(true))),
            TokenKind::False => Ok(Expr::Literal(Literal::Bool(false))),
            TokenKind::Identifier => Ok(Expr::Identifier(token.lexeme)),
            TokenKind::LeftParen => {
                self.skip_newlines();
                let expr = self.parse_expression()?;
                self.skip_newlines();
                self.expect(TokenKind::RightParen, "expected )")?;
                Ok(Expr::Grouping(Box::new(expr)))
            }
            _ => Err(ParseError { message: "expected expression".into(), span: token.span }),
        }
    }

    fn consume_statement_terminator(&mut self) -> Result<(), ParseError> {
        if self.match_kind(TokenKind::Semicolon) || self.match_kind(TokenKind::Newline) {
            self.skip_newlines();
            return Ok(());
        }
        if self.check(TokenKind::RightBrace) || self.check(TokenKind::Eof) { return Ok(()); }
        Err(self.error_here("expected end of statement"))
    }

    fn synchronize(&mut self) {
        while !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) || self.match_kind(TokenKind::Semicolon) {
                self.skip_newlines();
                return;
            }
            if matches!(self.peek().kind, TokenKind::Fn | TokenKind::Struct | TokenKind::RightBrace) { return; }
            self.advance();
        }
    }

    fn skip_newlines(&mut self) { while self.match_kind(TokenKind::Newline) {} }

    fn expect_identifier(&mut self, message: &str) -> Result<String, ParseError> {
        if self.check(TokenKind::Identifier) { Ok(self.advance().lexeme.clone()) } else { Err(self.error_here(message)) }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<Token, ParseError> {
        if self.check(kind) { Ok(self.advance().clone()) } else { Err(self.error_here(message)) }
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) { self.advance(); true } else { false }
    }

    fn check(&self, kind: TokenKind) -> bool { self.peek().kind == kind }

    fn peek(&self) -> &Token { &self.tokens[self.current] }

    fn advance(&mut self) -> &Token {
        if !self.check(TokenKind::Eof) { self.current += 1; }
        &self.tokens[self.current.saturating_sub(1)]
    }

    fn error_here(&self, message: &str) -> ParseError {
        ParseError { message: message.into(), span: self.peek().span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    #[test]
    fn parses_hello_program() {
        let program = parse("fn main() {\nlet message: str = \"Hello\"\nprintln(message)\n}");
        assert_eq!(program.items.len(), 1);
        match &program.items[0] {
            Item::Function(function) => {
                assert_eq!(function.name, "main");
                assert_eq!(function.body.statements.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parses_multiline_call() {
        let program = parse("fn main() {\nlet result = foo(\n10,\n20\n)\n}");
        assert_eq!(program.items.len(), 1);
    }

    #[test]
    fn parses_c_style_for() {
        let program = parse("fn main() {\nfor (let i = 0; i < 10; i++) {\nprintln(i)\n}\n}");
        match &program.items[0] {
            Item::Function(function) => assert!(matches!(function.body.statements[0], Stmt::For { .. })),
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parses_control_flow() {
        let program = parse("fn main() {\nlet x = 10\nif x > 0 {\nprintln(x)\n} else {\nprintln(0)\n}\nwhile x > 0 {\nprintln(x)\n}\n}");
        match &program.items[0] {
            Item::Function(function) => assert_eq!(function.body.statements.len(), 3),
            _ => panic!("expected function"),
        }
    }
}
