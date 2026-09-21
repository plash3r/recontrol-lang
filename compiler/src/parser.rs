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
            let start = self.current;
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(error) => {
                    errors.push(error);
                    break;
                }
            }
            if self.current == start && !self.check(TokenKind::Eof) {
                self.advance();
            }
            self.skip_newlines();
        }

        if errors.is_empty() { Ok(Program { items }) } else { Err(errors) }
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek().kind {
            TokenKind::Fn => self.parse_function().map(Item::Function),
            TokenKind::Struct => self.parse_struct().map(Item::Struct),
            TokenKind::Pub => match self.tokens.get(self.current + 1).map(|token| &token.kind) {
                Some(TokenKind::Fn) => self.parse_function().map(Item::Function),
                Some(TokenKind::Struct) => self.parse_struct().map(Item::Struct),
                _ => Err(self.error_here("pub is currently supported on fn and struct items")),
            },
            TokenKind::Impl => self.parse_impl().map(Item::Impl),
            TokenKind::Use => self.parse_import(),
            _ => Err(self.error_here("expected use, fn, struct, impl, or pub")),
        }
    }

    fn parse_import(&mut self) -> Result<Item, ParseError> {
        let start = self.expect(TokenKind::Use, "expected use")?.span;
        let path = self.expect(TokenKind::String, "expected string path after use")?;
        Ok(Item::Import(Import {
            path: path.lexeme,
            span: start.join(path.span),
        }))
    }

    fn parse_impl(&mut self) -> Result<ImplBlock, ParseError> {
        let start = self.expect(TokenKind::Impl, "expected impl")?.span;
        let type_name = self.expect_identifier_token("expected type name after impl")?.lexeme;
        self.skip_newlines();
        self.expect(TokenKind::LeftBrace, "expected { after impl type")?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            methods.push(self.parse_function()?);
            self.skip_newlines();
        }

        let end = self.expect(TokenKind::RightBrace, "expected } after impl block")?.span;
        Ok(ImplBlock { type_name, methods, span: start.join(end) })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let visibility = if self.check(TokenKind::Pub) {
            Some(self.advance().span)
        } else {
            None
        };
        let fn_token = self.expect(TokenKind::Fn, "expected fn")?;
        let start = visibility.unwrap_or(fn_token.span);
        let public = visibility.is_some();
        let name = self.expect_identifier_token("expected function name")?.lexeme;
        self.expect(TokenKind::LeftParen, "expected ( after function name")?;
        let mut params = Vec::new();

        if !self.check(TokenKind::RightParen) {
            loop {
                if self.check(TokenKind::Ampersand) {
                    let param_start = self.peek().span;
                    self.advance();
                    let reference = if self.match_kind(TokenKind::Mut) {
                        ReferenceKind::Mutable
                    } else {
                        ReferenceKind::Shared
                    };
                    let name_token = self.expect_identifier_token("expected receiver name")?;
                    let ty_span = param_start.join(name_token.span);
                    let ty = TypeRef {
                        name: "Self".into(),
                        reference,
                        array_len: None,
                        span: ty_span,
                    };
                    params.push(Parameter { name: name_token.lexeme, ty, span: ty_span });
                } else {
                    let name_token = self.expect_identifier_token("expected parameter name")?;
                    self.expect(TokenKind::Colon, "expected : after parameter name")?;
                    let ty = self.parse_type()?;
                    let span = name_token.span.join(ty.span);
                    params.push(Parameter { name: name_token.lexeme, ty, span });
                }
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
        let span = start.join(body.span);
        Ok(Function { public, name, params, return_type, body, span })
    }

    fn parse_struct(&mut self) -> Result<StructDef, ParseError> {
        let visibility = if self.check(TokenKind::Pub) {
            Some(self.advance().span)
        } else {
            None
        };
        let struct_token = self.expect(TokenKind::Struct, "expected struct")?;
        let start = visibility.unwrap_or(struct_token.span);
        let public = visibility.is_some();
        let name = self.expect_identifier_token("expected struct name")?.lexeme;
        self.skip_newlines();
        self.expect(TokenKind::LeftBrace, "expected { after struct name")?;
        let mut fields = Vec::new();
        self.skip_newlines();

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            let field_public = self.match_kind(TokenKind::Pub);
            let field_name = self.expect_identifier_token("expected field name")?;
            self.expect(TokenKind::Colon, "expected : after field name")?;
            let ty = self.parse_type()?;
            let span = field_name.span.join(ty.span);
            fields.push(Field { public: field_public, name: field_name.lexeme, ty, span });

            if self.match_kind(TokenKind::Comma) || self.match_kind(TokenKind::Semicolon) {
                self.skip_newlines();
            } else if self.check(TokenKind::Newline) {
                self.skip_newlines();
            } else if !self.check(TokenKind::RightBrace) {
                return Err(self.error_here("expected newline, comma, semicolon, or } after field"));
            }
        }

        let end = self.expect(TokenKind::RightBrace, "expected } after struct fields")?.span;
        Ok(StructDef { public, name, fields, span: start.join(end) })
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start = self.expect(TokenKind::LeftBrace, "expected {")?.span;
        self.skip_newlines();
        let mut statements = Vec::new();

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            statements.push(self.parse_statement()?);
            self.consume_statement_terminator()?;
            self.skip_newlines();
        }

        let end = self.expect(TokenKind::RightBrace, "expected }")?.span;
        Ok(Block { statements, span: start.join(end) })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().kind {
            TokenKind::Let => self.parse_let(false),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Return => self.parse_return(),
            TokenKind::LeftBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Ok(Stmt::new(StmtKind::Block(block), span))
            }
            _ => {
                let expression = self.parse_expression()?;
                let span = expression.span;
                Ok(Stmt::new(StmtKind::Expr(expression), span))
            }
        }
    }

    fn parse_let(&mut self, _in_for: bool) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::Let, "expected let")?.span;
        let mutable = self.match_kind(TokenKind::Mut);
        let name = self.expect_identifier_token("expected variable name")?.lexeme;
        let ty = if self.match_kind(TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        let initializer = if self.match_kind(TokenKind::Equal) { Some(self.parse_expression()?) } else { None };

        if ty.is_none() && initializer.is_none() {
            return Err(self.error_here("variable declaration needs a type or initializer"));
        }
        let span = self.span_from(start);
        Ok(Stmt::new(StmtKind::Let { name, mutable, ty, initializer }, span))
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::If, "expected if")?.span;
        let condition = self.parse_expression()?;
        self.skip_newlines();
        let then_branch = self.parse_block()?;
        self.skip_newlines();

        let else_branch = if self.match_kind(TokenKind::Else) {
            self.skip_newlines();
            if self.check(TokenKind::If) {
                Some(Box::new(self.parse_if()?))
            } else {
                let block = self.parse_block()?;
                let span = block.span;
                Some(Box::new(Stmt::new(StmtKind::Block(block), span)))
            }
        } else { None };

        let span = self.span_from(start);
        Ok(Stmt::new(StmtKind::If { condition, then_branch, else_branch }, span))
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::While, "expected while")?.span;
        let condition = self.parse_expression()?;
        self.skip_newlines();
        let body = self.parse_block()?;
        let span = start.join(body.span);
        Ok(Stmt::new(StmtKind::While { condition, body }, span))
    }

    fn parse_do_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::Do, "expected do")?.span;
        self.skip_newlines();
        let body = self.parse_block()?;
        self.skip_newlines();
        self.expect(TokenKind::While, "expected while after do block")?;
        let condition = self.parse_expression()?;
        let span = start.join(condition.span);
        Ok(Stmt::new(StmtKind::DoWhile { body, condition }, span))
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::For, "expected for")?.span;
        self.expect(TokenKind::LeftParen, "expected ( after for")?;
        self.skip_newlines();

        let initializer = if self.check(TokenKind::Semicolon) {
            None
        } else if self.check(TokenKind::Let) {
            Some(Box::new(self.parse_let(true)?))
        } else {
            let expression = self.parse_expression()?;
            let span = expression.span;
            Some(Box::new(Stmt::new(StmtKind::Expr(expression), span)))
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
        let span = start.join(body.span);
        Ok(Stmt::new(StmtKind::For { initializer, condition, update, body }, span))
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        let start = self.expect(TokenKind::Return, "expected return")?.span;
        let value = if self.check(TokenKind::Newline)
            || self.check(TokenKind::Semicolon)
            || self.check(TokenKind::RightBrace)
            || matches!(
                self.peek().kind,
                TokenKind::Let | TokenKind::If | TokenKind::While | TokenKind::Do |
                TokenKind::For | TokenKind::Return | TokenKind::LeftBrace
            )
        {
            None
        } else {
            Some(self.parse_expression()?)
        };
        let span = value.as_ref().map(|x| start.join(x.span)).unwrap_or(start);
        Ok(Stmt::new(StmtKind::Return(value), span))
    }

    fn parse_type(&mut self) -> Result<TypeRef, ParseError> {
        let start = self.peek().span;
        let reference = if self.match_kind(TokenKind::Ampersand) {
            if self.match_kind(TokenKind::Mut) {
                ReferenceKind::Mutable
            } else {
                ReferenceKind::Shared
            }
        } else {
            ReferenceKind::Value
        };
        let name = self.expect_identifier_token("expected type name")?.lexeme;
        let array_len = if self.match_kind(TokenKind::LeftBracket) {
            let length = self.expect(TokenKind::Number, "expected array length")?.lexeme.parse::<usize>()
                .map_err(|_| self.error_here("array length must be a non-negative integer"))?;
            self.expect(TokenKind::RightBracket, "expected ] after array length")?;
            Some(length)
        } else {
            None
        };
        Ok(TypeRef { name, reference, array_len, span: self.span_from(start) })
    }

    fn parse_expression(&mut self) -> Result<Expr, ParseError> { self.parse_assignment() }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_binary(0)?;
        let op = match self.peek().kind {
            TokenKind::Equal => Some(AssignOp::Assign),
            TokenKind::PlusEqual => Some(AssignOp::Add),
            TokenKind::MinusEqual => Some(AssignOp::Subtract),
            TokenKind::StarEqual => Some(AssignOp::Multiply),
            TokenKind::SlashEqual => Some(AssignOp::Divide),
            TokenKind::PercentEqual => Some(AssignOp::Modulo),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let value = self.parse_assignment()?;
            let span = left.span.join(value.span);
            return Ok(Expr::new(ExprKind::Assignment {
                target: Box::new(left),
                op,
                value: Box::new(value),
            }, span));
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
            let span = left.span.join(right.span);
            left = Expr::new(ExprKind::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }, span);
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
        let start = self.peek().span;
        let op = match self.peek().kind {
            TokenKind::Bang => { self.advance(); Some(UnaryOp::Not) }
            TokenKind::Minus => { self.advance(); Some(UnaryOp::Minus) }
            TokenKind::Plus => { self.advance(); Some(UnaryOp::Plus) }
            TokenKind::Ampersand => {
                self.advance();
                if self.match_kind(TokenKind::Mut) {
                    Some(UnaryOp::BorrowMutable)
                } else {
                    Some(UnaryOp::BorrowShared)
                }
            }
            _ => None,
        };

        if let Some(op) = op {
            let expression = self.parse_unary()?;
            let span = start.join(expression.span);
            return Ok(Expr::new(ExprKind::Unary { op, expr: Box::new(expression) }, span));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek().kind {
                TokenKind::LeftParen => {
                    let start = expr.span;
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

                    let end = self.expect(TokenKind::RightParen, "expected ) after arguments")?.span;
                    expr = Expr::new(ExprKind::Call { callee: Box::new(expr), args }, start.join(end));
                }
                TokenKind::LeftBracket => {
                    let start = expr.span;
                    self.advance();
                    self.skip_newlines();
                    let index = self.parse_expression()?;
                    self.skip_newlines();
                    let end = self.expect(TokenKind::RightBracket, "expected ] after index")?.span;
                    expr = Expr::new(ExprKind::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    }, start.join(end));
                }
                TokenKind::Dot => {
                    let start = expr.span;
                    self.advance();
                    let name = self.expect_identifier_token("expected member name after dot")?;
                    let span = start.join(name.span);
                    expr = Expr::new(ExprKind::Member { object: Box::new(expr), name: name.lexeme }, span);
                }
                TokenKind::PlusPlus => {
                    let start = expr.span;
                    let end = self.advance().span;
                    expr = Expr::new(ExprKind::Postfix { expr: Box::new(expr), op: PostfixOp::Increment }, start.join(end));
                }
                TokenKind::MinusMinus => {
                    let start = expr.span;
                    let end = self.advance().span;
                    expr = Expr::new(ExprKind::Postfix { expr: Box::new(expr), op: PostfixOp::Decrement }, start.join(end));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Number => Ok(Expr::new(ExprKind::Literal(Literal::Number(token.lexeme)), token.span)),
            TokenKind::String => Ok(Expr::new(ExprKind::Literal(Literal::String(token.lexeme)), token.span)),
            TokenKind::True => Ok(Expr::new(ExprKind::Literal(Literal::Bool(true)), token.span)),
            TokenKind::False => Ok(Expr::new(ExprKind::Literal(Literal::Bool(false)), token.span)),
            TokenKind::Identifier => {
                if self.looks_like_struct_literal() {
                    self.parse_struct_literal(token.lexeme, token.span)
                } else {
                    Ok(Expr::new(ExprKind::Identifier(token.lexeme), token.span))
                }
            }
            TokenKind::LeftBracket => self.parse_array_literal(token.span),
            TokenKind::LeftParen => {
                self.skip_newlines();
                let expr = self.parse_expression()?;
                self.skip_newlines();
                let end = self.expect(TokenKind::RightParen, "expected )")?.span;
                Ok(Expr::new(ExprKind::Grouping(Box::new(expr)), token.span.join(end)))
            }
            _ => Err(ParseError { message: "expected expression".into(), span: token.span }),
        }
    }

    fn looks_like_struct_literal(&self) -> bool {
        if !self.check(TokenKind::LeftBrace) {
            return false;
        }

        let mut index = self.current + 1;
        while self.tokens.get(index).map(|token| token.kind == TokenKind::Newline).unwrap_or(false) {
            index += 1;
        }

        match self.tokens.get(index).map(|token| &token.kind) {
            Some(TokenKind::RightBrace) => true,
            Some(TokenKind::Identifier) => {
                let mut next = index + 1;
                while self.tokens.get(next).map(|token| token.kind == TokenKind::Newline).unwrap_or(false) {
                    next += 1;
                }
                self.tokens.get(next).map(|token| token.kind == TokenKind::Colon).unwrap_or(false)
            }
            _ => false,
        }
    }

    fn parse_struct_literal(&mut self, name: String, start: Span) -> Result<Expr, ParseError> {
        self.expect(TokenKind::LeftBrace, "expected { in struct literal")?;
        self.skip_newlines();
        let mut fields = Vec::new();

        while !self.check(TokenKind::RightBrace) && !self.check(TokenKind::Eof) {
            let field = self.expect_identifier_token("expected field name")?.lexeme;
            self.expect(TokenKind::Colon, "expected : after field name")?;
            let value = self.parse_expression()?;
            fields.push((field, value));
            self.skip_newlines();
            if !self.match_kind(TokenKind::Comma) { break; }
            self.skip_newlines();
        }

        let end = self.expect(TokenKind::RightBrace, "expected } after struct literal")?.span;
        Ok(Expr::new(ExprKind::StructLiteral { name, fields }, start.join(end)))
    }

    fn parse_array_literal(&mut self, start: Span) -> Result<Expr, ParseError> {
        self.skip_newlines();
        let mut elements = Vec::new();

        if !self.check(TokenKind::RightBracket) {
            loop {
                elements.push(self.parse_expression()?);
                self.skip_newlines();
                if !self.match_kind(TokenKind::Comma) { break; }
                self.skip_newlines();
            }
        }

        let end = self.expect(TokenKind::RightBracket, "expected ] after array literal")?.span;
        Ok(Expr::new(ExprKind::Array(elements), start.join(end)))
    }

    fn consume_statement_terminator(&mut self) -> Result<(), ParseError> {
        if self.match_kind(TokenKind::Semicolon) || self.match_kind(TokenKind::Newline) {
            self.skip_newlines();
            return Ok(());
        }
        if self.check(TokenKind::RightBrace) || self.check(TokenKind::Eof) { return Ok(()); }
        if matches!(
            self.peek().kind,
            TokenKind::Let | TokenKind::If | TokenKind::While | TokenKind::Do | TokenKind::For |
            TokenKind::Return | TokenKind::LeftBrace | TokenKind::Identifier | TokenKind::Number |
            TokenKind::String | TokenKind::True | TokenKind::False | TokenKind::Bang | TokenKind::Minus |
            TokenKind::Plus | TokenKind::Ampersand | TokenKind::LeftBracket | TokenKind::LeftParen
        ) {
            return Ok(());
        }
        Err(self.error_here("expected end of statement"))
    }

    fn synchronize(&mut self) {
        while !self.check(TokenKind::Eof) {
            if self.match_kind(TokenKind::Newline) || self.match_kind(TokenKind::Semicolon) {
                self.skip_newlines();
                return;
            }
            if matches!(self.peek().kind, TokenKind::Fn | TokenKind::Struct | TokenKind::Impl | TokenKind::Use | TokenKind::Pub) { return; }
            if self.check(TokenKind::RightBrace) {
                self.advance();
                return;
            }
            self.advance();
        }
    }

    fn skip_newlines(&mut self) { while self.match_kind(TokenKind::Newline) {} }

    fn expect_identifier_token(&mut self, message: &str) -> Result<Token, ParseError> {
        self.expect(TokenKind::Identifier, message)
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<Token, ParseError> {
        if self.check(kind) { Ok(self.advance().clone()) } else { Err(self.error_here(message)) }
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) { self.advance(); true } else { false }
    }

    fn check(&self, kind: TokenKind) -> bool { self.peek().kind == kind }

    fn peek(&self) -> &Token { &self.tokens[self.current] }

    fn previous_span(&self) -> Span {
        self.tokens[self.current.saturating_sub(1)].span
    }

    fn span_from(&self, start: Span) -> Span { start.join(self.previous_span()) }

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
            Item::Function(function) => assert!(matches!(function.body.statements[0].kind, StmtKind::For { .. })),
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

    #[test]
    fn parses_visibility() {
        let program = parse("pub struct Point { pub x: i32, y: i32 }\npub fn make(): i32 { return 1 }");
        match &program.items[0] {
            Item::Struct(structure) => {
                assert!(structure.public);
                assert!(structure.fields[0].public);
                assert!(!structure.fields[1].public);
            }
            _ => panic!("expected struct"),
        }
        match &program.items[1] {
            Item::Function(function) => assert!(function.public),
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn spans_track_source_id() {
        let tokens = Lexer::with_source_id("fn main() {}", 7).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        assert_eq!(program.items[0].span().source_id, 7);
    }
}
