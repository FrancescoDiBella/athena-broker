use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    StringLit(String),
    NumberLit(f64),
    BoolLit(bool),
    Equal,           // ==
    NotEqual,        // !=
    Greater,         // >
    GreaterEqual,    // >=
    Less,            // <
    LessEqual,       // <=
    PatternMatch,    // ~=
    NotPatternMatch, // !~=
    And,             // ;
    Or,              // |
    LParen,          // (
    RParen,          // )
    LBracket,        // [
    RBracket,        // ]
    Comma,           // ,
    DotDot,          // ..
    Eof,
}

#[derive(Debug, Error, PartialEq)]
pub enum LexerError {
    #[error("Unexpected character '{0}' at position {1}")]
    UnexpectedChar(char, usize),

    #[error("Unterminated string literal at position {0}")]
    UnterminatedString(usize),

    #[error("Invalid number format at position {0}")]
    InvalidNumber(usize),
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Vec<(usize, char)>,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let chars: Vec<(usize, char)> = input.char_indices().collect();
        Self {
            input,
            chars,
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).map(|&(_, c)| c)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).map(|&(_, c)| c)
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.chars.len() {
            let ch = self.chars[self.pos].1;
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn current_pos(&self) -> usize {
        self.chars
            .get(self.pos)
            .map(|&(p, _)| p)
            .unwrap_or(self.input.len())
    }

    pub fn next_token(&mut self) -> Result<Token, LexerError> {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }

        let start_pos = self.current_pos();
        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match c {
            ';' => Ok(Token::And),
            '|' => Ok(Token::Or),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '[' => Ok(Token::LBracket),
            ']' => Ok(Token::RBracket),
            ',' => Ok(Token::Comma),
            '.' if self.peek() == Some('.') => {
                self.advance();
                Ok(Token::DotDot)
            }
            '=' if self.peek() == Some('=') => {
                self.advance();
                Ok(Token::Equal)
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::NotEqual)
                } else if self.peek() == Some('~') && self.peek_next() == Some('=') {
                    self.advance();
                    self.advance();
                    Ok(Token::NotPatternMatch)
                } else {
                    Err(LexerError::UnexpectedChar('!', start_pos))
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::GreaterEqual)
                } else {
                    Ok(Token::Greater)
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::LessEqual)
                } else {
                    Ok(Token::Less)
                }
            }
            '~' if self.peek() == Some('=') => {
                self.advance();
                Ok(Token::PatternMatch)
            }
            '"' | '\'' => {
                let quote = c;
                let mut string_val = String::new();
                let mut closed = false;

                while let Some(ch) = self.advance() {
                    if ch == '\\' {
                        if let Some(escaped) = self.advance() {
                            string_val.push(escaped);
                        }
                    } else if ch == quote {
                        closed = true;
                        break;
                    } else {
                        string_val.push(ch);
                    }
                }

                if !closed {
                    return Err(LexerError::UnterminatedString(start_pos));
                }
                Ok(Token::StringLit(string_val))
            }
            _ if c.is_ascii_digit()
                || (c == '-' && self.peek().map(|ch| ch.is_ascii_digit()).unwrap_or(false)) =>
            {
                let mut num_str = String::new();
                num_str.push(c);
                let mut has_dot = false;

                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        num_str.push(ch);
                        self.advance();
                    } else if ch == '.' && !has_dot && self.peek_next() != Some('.') {
                        has_dot = true;
                        num_str.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let num: f64 = num_str
                    .parse()
                    .map_err(|_| LexerError::InvalidNumber(start_pos))?;
                if !num.is_finite() {
                    return Err(LexerError::InvalidNumber(start_pos));
                }
                Ok(Token::NumberLit(num))
            }
            _ if c.is_alphabetic() || c == '_' || c == '$' => {
                let mut ident = String::new();
                ident.push(c);

                while let Some(ch) = self.peek() {
                    // Property paths can have dots, underscores, colons, hyphens
                    if ch.is_alphanumeric()
                        || ch == '_'
                        || ch == '.'
                        || ch == ':'
                        || ch == '-'
                        || ch == '/'
                        || ch == '#'
                        || ch == '%'
                    {
                        ident.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }

                match ident.as_str() {
                    "true" => Ok(Token::BoolLit(true)),
                    "false" => Ok(Token::BoolLit(false)),
                    _ => Ok(Token::Ident(ident)),
                }
            }
            _ => Err(LexerError::UnexpectedChar(c, start_pos)),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            if tok == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        Ok(tokens)
    }
}
