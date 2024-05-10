use std::ops::{Deref, DerefMut};

use bytemuck::{CheckedBitPattern, NoUninit};
use serde::{Serialize, Serializer};
use serde::ser::SerializeSeq;

use crate::AnyBitPatternArray;

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct CheckedArray<T, const N: usize> {
  data: [T; N],
}

unsafe impl<T, const N: usize> CheckedBitPattern for CheckedArray<T, N>
  where
    T: CheckedBitPattern,
{
  type Bits = AnyBitPatternArray<T::Bits, N>;

  fn is_valid_bit_pattern(bits: &Self::Bits) -> bool {
    bits.iter().all(|b| T::is_valid_bit_pattern(b))
  }
}

impl<T, const N: usize> Deref for CheckedArray<T, N> {
  type Target = [T; N];

  fn deref(&self) -> &[T; N] {
    &self.data
  }
}

impl<T, const N: usize> DerefMut for CheckedArray<T, N> {
  fn deref_mut(&mut self) -> &mut [T; N] {
    &mut self.data
  }
}

unsafe impl<T, const N: usize> NoUninit for CheckedArray<T, N> where T: NoUninit + Copy + Clone {}

impl<T, const N: usize> Serialize for CheckedArray<T, N>
  where
    T: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                  where
                    S: Serializer,
  {
    // Initialize a serialization sequence
    let mut seq = serializer.serialize_seq(Some(N))?;
    for item in &self.data {
      seq.serialize_element(item)?;
    }
    seq.end()
  }
}