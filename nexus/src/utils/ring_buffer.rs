use std::collections::VecDeque;

/// Last "capacity" data from current datum.
/// 0th index is the newest datum, last index is oldest datum.
#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
  pub vec: VecDeque<T>,
  pub capacity: usize,
  pub id: String,
}

impl<T: Clone> RingBuffer<T> {
  pub fn new(capacity: usize, id: String) -> Self {
    Self {
      vec: VecDeque::with_capacity(capacity),
      capacity,
      id,
    }
  }

  pub fn push(&mut self, t: T) {
    if self.vec.len() == self.capacity {
      self.vec.pop_back();
    }
    self.vec.push_front(t);
  }

  pub fn front(&self) -> Option<&T> {
    self.vec.front()
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

  pub fn find(&self, f: impl Fn(&T) -> bool) -> Option<&T> {
    self.vec.iter().find(|t| f(t))
  }

  /// 0th index is the oldest element.
  pub fn vec(&self) -> Vec<T> {
    self.vec.iter().rev().cloned().collect::<Vec<T>>()
  }

  /// 0th index is the newest element.
  pub fn rev_vec(&self) -> Vec<T> {
    self.vec.iter().cloned().collect::<Vec<T>>()
  }
}
