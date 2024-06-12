use indexmap::IndexMap;

pub struct RingMap<K, V> {
  map: IndexMap<K, V>,
  capacity: usize,
}

impl<K, V> RingMap<K, V>
where
  K: Eq + std::hash::Hash + Clone,
{
  /// Create a new ring buffer with the specified capacity
  pub fn new(capacity: usize) -> Self {
    RingMap {
      map: IndexMap::with_capacity(capacity),
      capacity,
    }
  }

  /// Insert a key-value pair into the ring buffer
  pub fn insert(&mut self, key: K, value: V) {
    if self.map.contains_key(&key) {
      // If the key already exists, just update the value
      self.map.insert(key, value);
    } else {
      if self.map.len() == self.capacity {
        // If the buffer is full, remove the oldest entry
        let oldest_key = self.map.keys().next().unwrap().clone();
        self.map.shift_remove(&oldest_key);
      }
      // Insert the new entry
      self.map.insert(key, value);
    }
  }

  pub fn get(&self, key: &K) -> Option<&V> {
    self.map.get(key)
  }

  pub fn remove(&mut self, key: &K) -> Option<V> {
    self.map.shift_remove(key)
  }

  pub fn len(&self) -> usize {
    self.map.len()
  }

  pub fn is_empty(&self) -> bool {
    self.map.is_empty()
  }

  pub fn newest(&self) -> Option<(&K, &V)> {
    self.map.last()
  }

  pub fn values(&self) -> impl Iterator<Item = &V> {
    self.map.values()
  }

  pub fn key_values(&self) -> impl Iterator<Item = (&K, &V)> {
    self.map.iter()
  }
}
