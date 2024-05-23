use std::ops::{Deref, DerefMut};

use bytemuck::{AnyBitPattern, NoUninit, Zeroable};

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct AnyBitPatternArray<T, const N: usize> {
  data: [T; N],
}

unsafe impl<T, const N: usize> Zeroable for AnyBitPatternArray<T, N> where
  T: AnyBitPattern + Clone + Copy
{}

unsafe impl<T, const N: usize> AnyBitPattern for AnyBitPatternArray<T, N> where
  T: AnyBitPattern + Copy + Clone
{}

impl<T, const N: usize> Deref for AnyBitPatternArray<T, N> {
  type Target = [T; N];

  fn deref(&self) -> &[T; N] {
    &self.data
  }
}

impl<T, const N: usize> DerefMut for AnyBitPatternArray<T, N> {
  fn deref_mut(&mut self) -> &mut [T; N] {
    &mut self.data
  }
}

unsafe impl<T, const N: usize> NoUninit for AnyBitPatternArray<T, N> where T: NoUninit + Copy + Clone
{}