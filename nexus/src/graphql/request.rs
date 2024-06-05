use serde::Serialize;

#[derive(Serialize)]
pub struct GraphqlRequest<Variables> {
  pub variables: Variables,
  pub query: &'static str,
  pub operation_name: &'static str,
}
