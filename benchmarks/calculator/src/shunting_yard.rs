use crate::{
    instruction::{BinaryOperator, Instruction, Output},
    lexer::{Lexer, Token},
    math_function::MathFunction,
    stack::Stack,
    CalcError,
};

const MAX_NESTING: usize = 32;

#[derive(Clone, Copy)]
enum Operator {
    Open,
    Function(MathFunction),
    Binary(BinaryOperator),
    Negate,
    Positive,
}

impl Operator {
    fn precedence(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Binary(operator) => operator.precedence(),
            Self::Negate | Self::Positive => 5,
            Self::Function(_) => 7,
        }
    }

    fn instruction(self) -> Result<Instruction, CalcError> {
        match self {
            Self::Open => Err(CalcError::Incomplete),
            Self::Function(function) => Ok(Instruction::Function(function)),
            Self::Binary(operator) => Ok(Instruction::Binary(operator)),
            Self::Negate => Ok(Instruction::Negate),
            Self::Positive => Ok(Instruction::Positive),
        }
    }
}

/// Both candidates share a tokenizer and precedence handling; only the output differs.
pub struct ShuntingYard {
    operators: Stack<Operator>,
    nesting: usize,
}

impl ShuntingYard {
    pub fn new() -> Self {
        Self {
            operators: Stack::new(Operator::Open),
            nesting: 0,
        }
    }

    pub fn parse(&mut self, input: &str, output: &mut impl Output) -> Result<(), CalcError> {
        self.operators.clear();
        self.nesting = 0;
        output.clear();
        if input.len() > core_engine::search::MAX_QUERY_BYTES {
            return Err(CalcError::TooComplex);
        }
        let mut lexer = Lexer::new(input);
        let mut needs_operand = true;
        while let Some(token) = lexer.next()? {
            needs_operand = self.consume(token, needs_operand, &mut lexer, output)?;
        }
        if needs_operand {
            return Err(CalcError::Incomplete);
        }
        while self.operators.last().is_some() {
            self.reduce(output)?;
        }
        Ok(())
    }

    fn consume(
        &mut self,
        token: Token,
        needs_operand: bool,
        lexer: &mut Lexer<'_>,
        output: &mut impl Output,
    ) -> Result<bool, CalcError> {
        match token {
            Token::Number(number) if needs_operand => output.emit(Instruction::Number(number))?,
            Token::Function(function) if needs_operand => {
                if !matches!(lexer.next()?, Some(Token::Open)) {
                    return Err(CalcError::Incomplete);
                }
                self.operators.push(Operator::Function(function))?;
                self.open()?;
                return Ok(true);
            }
            Token::Open if needs_operand => {
                self.open()?;
                return Ok(true);
            }
            Token::Close if !needs_operand => self.close(output)?,
            Token::Percent if !needs_operand => output.emit(Instruction::Percent)?,
            Token::Binary(BinaryOperator::Add | BinaryOperator::Subtract) if needs_operand => {
                self.operators.push(
                    if matches!(token, Token::Binary(BinaryOperator::Subtract)) {
                        Operator::Negate
                    } else {
                        Operator::Positive
                    },
                )?;
                return Ok(true);
            }
            Token::Binary(operator) if !needs_operand => {
                self.binary(operator, output)?;
                return Ok(true);
            }
            _ => return Err(CalcError::Invalid),
        }
        Ok(false)
    }

    fn open(&mut self) -> Result<(), CalcError> {
        self.nesting += 1;
        if self.nesting >= MAX_NESTING {
            return Err(CalcError::TooComplex);
        }
        self.operators.push(Operator::Open)
    }

    fn close(&mut self, output: &mut impl Output) -> Result<(), CalcError> {
        while self
            .operators
            .last()
            .is_some_and(|operator| !matches!(operator, Operator::Open))
        {
            self.reduce(output)?;
        }
        self.operators.pop()?;
        self.nesting = self.nesting.checked_sub(1).ok_or(CalcError::Invalid)?;
        if matches!(self.operators.last(), Some(Operator::Function(_))) {
            self.reduce(output)?;
        }
        Ok(())
    }

    fn binary(
        &mut self,
        incoming: BinaryOperator,
        output: &mut impl Output,
    ) -> Result<(), CalcError> {
        while self.operators.last().is_some_and(|operator| {
            operator.precedence() > incoming.precedence()
                || operator.precedence() == incoming.precedence() && !incoming.right_associative()
        }) {
            self.reduce(output)?;
        }
        self.operators.push(Operator::Binary(incoming))
    }

    fn reduce(&mut self, output: &mut impl Output) -> Result<(), CalcError> {
        output.emit(self.operators.pop()?.instruction()?)
    }
}
