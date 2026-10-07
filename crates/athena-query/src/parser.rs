use thiserror::Error;

use crate::ast::{CompareOp, Literal, LogicalOp, QueryExpr};
use crate::lexer::{Lexer, LexerError, Token};

#[derive(Debug, Error, PartialEq)]
pub enum ParserError {
    #[error("Query exceeds the complexity limit")]
    TooComplex,
    #[error("Invalid or unsupported regular expression: {0}")]
    InvalidPattern(String),
    #[error("Lexer error: {0}")]
    Lexer(#[from] LexerError),

    #[error("Unexpected token: {0:?}")]
    UnexpectedToken(Token),

    #[error("Unexpected end of input")]
    UnexpectedEof,

    #[error("Expected identifier, found {0:?}")]
    ExpectedIdentifier(Token),

    #[error("Expected comparison operator, found {0:?}")]
    ExpectedOperator(Token),

    #[error("Expected literal value, found {0:?}")]
    ExpectedLiteral(Token),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn validate_pattern(pattern: &str) -> Result<(), ParserError> {
        if pattern.len() > 16_384 {
            return Err(ParserError::TooComplex);
        }
        regex::Regex::new(pattern)
            .map(|_| ())
            .map_err(|error| ParserError::InvalidPattern(error.to_string()))
    }

    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_str(input: &str) -> Result<QueryExpr, ParserError> {
        if input.len() > 16_384 {
            return Err(ParserError::TooComplex);
        }
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        if tokens.len() > 512 {
            return Err(ParserError::TooComplex);
        }
        let mut depth = 0usize;
        for token in &tokens {
            if *token == Token::LParen {
                depth += 1;
            }
            if depth > 32 {
                return Err(ParserError::TooComplex);
            }
            if *token == Token::RParen {
                depth = depth.saturating_sub(1);
            }
        }
        let mut parser = Parser::new(tokens);
        let expr = parser.parse_expr()?;

        if parser.peek() != &Token::Eof {
            return Err(ParserError::UnexpectedToken(parser.peek().clone()));
        }

        Ok(expr)
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            tok
        } else {
            Token::Eof
        }
    }

    pub fn parse_expr(&mut self) -> Result<QueryExpr, ParserError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<QueryExpr, ParserError> {
        let mut left = self.parse_and()?;

        while self.peek() == &Token::Or {
            self.advance(); // consume '|'
            let right = self.parse_and()?;
            left = QueryExpr::Binary {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<QueryExpr, ParserError> {
        let mut left = self.parse_primary()?;

        while self.peek() == &Token::And {
            self.advance(); // consume ';'
            let right = self.parse_primary()?;
            left = QueryExpr::Binary {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<QueryExpr, ParserError> {
        match self.peek() {
            Token::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expr()?;
                if self.peek() == &Token::RParen {
                    self.advance(); // consume ')'
                    Ok(expr)
                } else {
                    Err(ParserError::UnexpectedToken(self.peek().clone()))
                }
            }
            Token::Ident(_) => self.parse_comparison(),
            other => Err(ParserError::UnexpectedToken(other.clone())),
        }
    }

    fn parse_comparison(&mut self) -> Result<QueryExpr, ParserError> {
        let path = match self.advance() {
            Token::Ident(s) => s,
            other => return Err(ParserError::ExpectedIdentifier(other)),
        };

        match self.peek() {
            Token::PatternMatch => {
                self.advance();
                let pattern = self.parse_string_value()?;
                Self::validate_pattern(&pattern)?;
                Ok(QueryExpr::PatternMatch {
                    path,
                    pattern,
                    negated: false,
                })
            }
            Token::NotPatternMatch => {
                self.advance();
                let pattern = self.parse_string_value()?;
                Self::validate_pattern(&pattern)?;
                Ok(QueryExpr::PatternMatch {
                    path,
                    pattern,
                    negated: true,
                })
            }
            Token::Equal => {
                self.advance();
                let bracketed = self.peek() == &Token::LBracket;
                if bracketed {
                    self.advance();
                }
                let first = self.parse_literal()?;
                let expression = if self.peek() == &Token::DotDot {
                    self.advance();
                    QueryExpr::Range {
                        path,
                        min: first,
                        max: self.parse_literal()?,
                    }
                } else if self.peek() == &Token::Comma {
                    let mut values = vec![first];
                    while self.peek() == &Token::Comma {
                        self.advance();
                        values.push(self.parse_literal()?);
                    }
                    QueryExpr::InList { path, values }
                } else if bracketed {
                    QueryExpr::InList {
                        path,
                        values: vec![first],
                    }
                } else {
                    QueryExpr::Comparison {
                        path,
                        op: CompareOp::Equal,
                        value: first,
                    }
                };
                if bracketed {
                    if self.advance() != Token::RBracket {
                        return Err(ParserError::UnexpectedToken(self.peek().clone()));
                    }
                }
                Ok(expression)
            }

            Token::NotEqual => {
                self.advance();
                let value = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::NotEqual,
                    value,
                })
            }
            Token::Greater => {
                self.advance();
                let value = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::GreaterThan,
                    value,
                })
            }
            Token::GreaterEqual => {
                self.advance();
                let value = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::GreaterThanOrEqual,
                    value,
                })
            }
            Token::Less => {
                self.advance();
                let value = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::LessThan,
                    value,
                })
            }
            Token::LessEqual => {
                self.advance();
                let value = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::LessThanOrEqual,
                    value,
                })
            }
            other => Err(ParserError::ExpectedOperator(other.clone())),
        }
    }

    fn parse_string_value(&mut self) -> Result<String, ParserError> {
        match self.advance() {
            Token::StringLit(s) => Ok(s),
            Token::Ident(s) => Ok(s),
            other => Err(ParserError::ExpectedLiteral(other)),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, ParserError> {
        match self.advance() {
            Token::NumberLit(n) => Ok(Literal::Number(n)),
            Token::StringLit(s) => Ok(Literal::String(s)),
            Token::BoolLit(b) => Ok(Literal::Boolean(b)),
            Token::Ident(s) => Ok(Literal::String(s)),
            other => Err(ParserError::ExpectedLiteral(other)),
        }
    }
}
