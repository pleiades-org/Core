use crate::{
    instruction::{finite, BinaryOperator},
    math_function::{constant, MathFunction},
    CalcError,
};

#[derive(Clone, Copy)]
pub enum Token {
    Number(f64),
    Binary(BinaryOperator),
    Open,
    Close,
    Percent,
    Function(MathFunction),
}

pub struct Lexer<'input> {
    bytes: &'input [u8],
    cursor: usize,
    tokens: usize,
}

impl<'input> Lexer<'input> {
    pub fn new(input: &'input str) -> Self {
        Self {
            bytes: input.as_bytes(),
            cursor: 0,
            tokens: 0,
        }
    }

    pub fn next(&mut self) -> Result<Option<Token>, CalcError> {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
        let Some(&byte) = self.bytes.get(self.cursor) else {
            return Ok(None);
        };
        self.tokens += 1;
        if self.tokens > crate::stack::CAPACITY {
            return Err(CalcError::TooComplex);
        }
        let token = match byte {
            b'0'..=b'9' | b'.' => return self.number().map(|number| Some(Token::Number(number))),
            b'o' if self.bytes.get(self.cursor..self.cursor + 2) == Some(b"of") => {
                self.cursor += 1;
                Token::Binary(BinaryOperator::Multiply)
            }
            b'a'..=b'z' | b'A'..=b'Z' => return self.named().map(Some),
            b'+' => Token::Binary(BinaryOperator::Add),
            b'-' => Token::Binary(BinaryOperator::Subtract),
            b'*' => Token::Binary(BinaryOperator::Multiply),
            b'/' => Token::Binary(BinaryOperator::Divide),
            b'^' => Token::Binary(BinaryOperator::Power),
            b'(' => Token::Open,
            b')' => Token::Close,
            b'%' => Token::Percent,
            _ => return Err(CalcError::Invalid),
        };
        self.cursor += 1;
        Ok(Some(token))
    }

    fn named(&mut self) -> Result<Token, CalcError> {
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_alphanumeric)
        {
            self.cursor += 1;
        }
        let name = &self.bytes[start..self.cursor];
        if let Some(number) = constant(name) {
            return Ok(Token::Number(number));
        }
        MathFunction::parse(name)
            .map(Token::Function)
            .ok_or(CalcError::Invalid)
    }

    fn number(&mut self) -> Result<f64, CalcError> {
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
        }
        if self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            self.cursor += 1;
            if self
                .bytes
                .get(self.cursor)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.cursor += 1;
            }
            let exponent_start = self.cursor;
            while self.bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
            if self.cursor == exponent_start {
                return Err(CalcError::Incomplete);
            }
        }
        let text =
            std::str::from_utf8(&self.bytes[start..self.cursor]).map_err(|_| CalcError::Invalid)?;
        finite(text.parse().map_err(|_| CalcError::Invalid)?)
    }
}
