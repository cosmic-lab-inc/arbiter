use std::panic::Location;

pub trait SafeUnwrap {
  type Item;

  fn safe_unwrap(self) -> anyhow::Result<Self::Item>;
}

impl<T> SafeUnwrap for Option<T> {
  type Item = T;

  #[track_caller]
  #[inline(always)]
  fn safe_unwrap(self) -> anyhow::Result<T> {
    match self {
      Some(v) => Ok(v),
      None => {
        let caller = Location::caller();
        Err(anyhow::anyhow!("Failed unwrap from: {:?}", caller))
      }
    }
  }
}

impl<T, U> SafeUnwrap for Result<T, U> {
  type Item = T;

  #[track_caller]
  #[inline(always)]
  fn safe_unwrap(self) -> anyhow::Result<T> {
    match self {
      Ok(v) => Ok(v),
      Err(_) => {
        let caller = Location::caller();
        Err(anyhow::anyhow!("Failed unwrap from: {:?}", caller))
      }
    }
  }
}