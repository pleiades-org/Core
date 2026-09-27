use core_engine::calculator::CalcError;

pub const CAPACITY: usize = 256;

/// Reusable bounded scratch space: reset only its length between expressions.
pub struct Stack<T: Copy> {
    items: [T; CAPACITY],
    length: usize,
}

impl<T: Copy> Stack<T> {
    pub fn new(initial: T) -> Self {
        Self {
            items: [initial; CAPACITY],
            length: 0,
        }
    }

    pub fn clear(&mut self) {
        self.length = 0;
    }

    pub fn push(&mut self, item: T) -> Result<(), CalcError> {
        if self.length == CAPACITY {
            return Err(CalcError::TooComplex);
        }
        self.items[self.length] = item;
        self.length += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<T, CalcError> {
        self.length = self.length.checked_sub(1).ok_or(CalcError::Invalid)?;
        Ok(self.items[self.length])
    }

    pub fn last(&self) -> Option<T> {
        self.length.checked_sub(1).map(|index| self.items[index])
    }

    pub fn entries(&self) -> &[T] {
        &self.items[..self.length]
    }
}
