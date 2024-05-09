use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChannelEvent<T> {
  Msg(T),
  Done
}