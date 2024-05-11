use std::panic::Location;

use crate::ceil_div::CheckedCeilDiv;
use crate::floor_div::CheckedFloorDiv;

pub trait SafeMath: Sized {
  fn safe_add(self, rhs: Self) -> anyhow::Result<Self>;
  fn safe_sub(self, rhs: Self) -> anyhow::Result<Self>;
  fn safe_mul(self, rhs: Self) -> anyhow::Result<Self>;
  fn safe_div(self, rhs: Self) -> anyhow::Result<Self>;
  fn safe_div_ceil(self, rhs: Self) -> anyhow::Result<Self>;
}

macro_rules! checked_impl {
    ($t:ty) => {
        impl SafeMath for $t {
            #[track_caller]
            #[inline(always)]
            fn safe_add(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_add(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }

            #[track_caller]
            #[inline(always)]
            fn safe_sub(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_sub(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }

            #[track_caller]
            #[inline(always)]
            fn safe_mul(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_mul(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }

            #[track_caller]
            #[inline(always)]
            fn safe_div(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_div(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }

            #[track_caller]
            #[inline(always)]
            fn safe_div_ceil(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_ceil_div(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }
        }
    };
}

checked_impl!(u128);
checked_impl!(u64);
checked_impl!(u32);
checked_impl!(u16);
checked_impl!(u8);
checked_impl!(i128);
checked_impl!(i64);
checked_impl!(i32);
checked_impl!(i16);
checked_impl!(i8);

pub trait SafeDivFloor: Sized {
  /// Perform floor division
  fn safe_div_floor(self, rhs: Self) -> anyhow::Result<Self>;
}

macro_rules! div_floor_impl {
    ($t:ty) => {
        impl SafeDivFloor for $t {
            #[track_caller]
            #[inline(always)]
            fn safe_div_floor(self, v: $t) -> anyhow::Result<$t> {
                match self.checked_floor_div(v) {
                    Some(result) => Ok(result),
                    None => {
                        let caller = Location::caller();
                        Err(anyhow::anyhow!("Failed to safe math"))
                    }
                }
            }
        }
    };
}

div_floor_impl!(i128);
div_floor_impl!(i64);
div_floor_impl!(i32);
div_floor_impl!(i16);
div_floor_impl!(i8);