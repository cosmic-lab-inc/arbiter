use crate::_User;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Location {
  pub line: i32,
  pub column: i32,
}

#[derive(Debug, Deserialize)]
pub enum PathFragment {
  Key(String),
  Index(i32),
}

#[derive(Debug, Deserialize)]
pub struct GraphqlError {
  pub message: String,
  #[serde(default)]
  pub locations: Option<Vec<Location>>,
  #[serde(default)]
  pub path: Option<Vec<PathFragment>>,
  #[serde(default)]
  pub extensions: Option<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub struct GraphqlResponse<T> {
  pub data: T,
  #[serde(default)]
  pub errors: Option<Vec<GraphqlError>>,
  // #[serde(default)]
  // pub extensions: Option<HashMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub struct DriftUsers {
  #[serde(rename = "drift_User")]
  pub drift_user: Vec<_User>,
}
