use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
  pub vec: VecDeque<T>,
  pub capacity: usize,
}

impl<T> RingBuffer<T> {
  pub fn new(capacity: usize) -> Self {
    Self {
      vec: VecDeque::with_capacity(capacity),
      capacity,
    }
  }

  pub fn push(&mut self, t: T) {
    if self.vec.len() == self.capacity {
      self.vec.pop_back();
    }
    self.vec.push_front(t);
  }

  pub fn full(&self) -> bool {
    self.vec.len() == self.capacity
  }

  pub fn take(&mut self) -> Vec<T> {
    self.vec.drain(..).collect()
  }

  pub fn is_empty(&self) -> bool {
    self.vec.is_empty()
  }

  pub fn len(&self) -> usize {
    self.vec.len()
  }
}