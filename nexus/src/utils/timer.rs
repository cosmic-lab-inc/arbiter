use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub struct Timer {
  start: Instant,
}

impl Timer {
  pub fn new() -> Self {
    Self {
      start: Instant::now(),
    }
  }

  pub fn seconds(&self) -> u64 {
    let elapsed = self.start.elapsed();
    elapsed.as_secs()
  }

  pub fn millis(&self) -> u128 {
    let elapsed = self.start.elapsed();
    elapsed.as_millis()
  }

  pub fn micros(&self) -> u128 {
    let elapsed = self.start.elapsed();
    elapsed.as_micros()
  }

  pub fn nanos(&self) -> u128 {
    let elapsed = self.start.elapsed();
    elapsed.as_nanos()
  }
}
