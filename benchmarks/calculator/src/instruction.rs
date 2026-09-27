use crate::{math_function::MathFunction, stack::Stack, CalcError};

#[derive(Clone, Copy)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

impl BinaryOperator {
    pub fn precedence(self) -> u8 {
        match self {
            Self::Add | Self::Subtract => 1,
            Self::Multiply | Self::Divide => 3,
            Self::Power => 6,
        }
    }

    pub fn right_associative(self) -> bool {
        matches!(self, Self::Power)
    }

    fn apply(self, left: f64, right: f64) -> Result<f64, CalcError> {
        finite(match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide if right == 0. => return Err(CalcError::DivisionByZero),
            Self::Divide => left / right,
            Self::Power => left.powf(right),
        })
    }
}

#[derive(Clone, Copy)]
pub enum Instruction {
    Number(f64),
    Binary(BinaryOperator),
    Negate,
    Positive,
    Percent,
    Function(MathFunction),
}

pub trait Output {
    fn clear(&mut self);
    fn emit(&mut self, instruction: Instruction) -> Result<(), CalcError>;
}

pub struct Values(Stack<f64>);

impl Values {
    pub fn new() -> Self {
        Self(Stack::new(0.))
    }

    pub fn result(&self) -> Result<f64, CalcError> {
        match self.0.entries() {
            [number] => finite(*number),
            _ => Err(CalcError::Invalid),
        }
    }
}

impl Output for Values {
    fn clear(&mut self) {
        self.0.clear();
    }

    fn emit(&mut self, instruction: Instruction) -> Result<(), CalcError> {
        let number = match instruction {
            Instruction::Number(number) => number,
            Instruction::Binary(operator) => {
                let right = self.0.pop()?;
                let left = self.0.pop()?;
                operator.apply(left, right)?
            }
            Instruction::Negate => -self.0.pop()?,
            Instruction::Positive => self.0.pop()?,
            Instruction::Percent => self.0.pop()? / 100.,
            Instruction::Function(function) => function.apply(self.0.pop()?)?,
        };
        self.0.push(finite(number)?)
    }
}

pub struct Program(Stack<Instruction>);

impl Program {
    pub fn new() -> Self {
        Self(Stack::new(Instruction::Number(0.)))
    }

    pub fn run(&self, values: &mut Values) -> Result<f64, CalcError> {
        values.clear();
        for instruction in self.0.entries() {
            values.emit(*instruction)?;
        }
        values.result()
    }
}

impl Output for Program {
    fn clear(&mut self) {
        self.0.clear();
    }
    fn emit(&mut self, instruction: Instruction) -> Result<(), CalcError> {
        self.0.push(instruction)
    }
}

pub fn finite(number: f64) -> Result<f64, CalcError> {
    if number.is_finite() {
        Ok(number)
    } else {
        Err(CalcError::NonFinite)
    }
}
