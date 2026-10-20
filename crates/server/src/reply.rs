//! How a route answers (docs/api.md, 「Wire format」): its own type, or, for a failure and a refusal alike,
//! the status its code is answered with and a body of `code` and `error`.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Query;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use bmo_node_client::{ErrorCode, WireError};
use serde::de::DeserializeOwned;
use store::StoreError;

/// What a route answers where it does not answer its own type.
#[derive(Debug)]
pub struct Reply(pub WireError);

impl Reply {
  pub fn new(code: ErrorCode, error: impl Into<String>) -> Self {
    Self(WireError::new(code, error))
  }

  pub fn bad_request(error: impl Into<String>) -> Self {
    Self::new(ErrorCode::BadRequest, error)
  }

  pub fn failed(error: impl Into<String>) -> Self {
    Self::new(ErrorCode::Failed, error)
  }

  pub fn not_found(error: impl Into<String>) -> Self {
    Self::new(ErrorCode::NotFound, error)
  }

  pub fn no_node(name: &str) -> Self {
    Self::not_found(format!("no node named {name}"))
  }

  pub fn bad_field(error: impl Into<String>) -> Self {
    Self::new(ErrorCode::BadField, error)
  }
}

impl IntoResponse for Reply {
  fn into_response(self) -> Response {
    let status = StatusCode::from_u16(self.0.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    (status, Json(self.0)).into_response()
  }
}

impl From<StoreError> for Reply {
  fn from(error: StoreError) -> Self {
    match error {
      StoreError::Refused(refusal) => Self::new(refusal.code, refusal.message),
      other => Self::failed(other.to_string()),
    }
  }
}

/// A route's answer: its own type, or a reply in the error shape.
pub type Answer<T> = Result<Json<T>, Reply>;

/// A route's answer for a thing it made: 201 and the thing, or a reply in the error shape.
pub type Made<T> = Result<(StatusCode, Json<T>), Reply>;

/// A route's answer where nothing is left to answer: 204, or a reply in the error shape.
pub type Done = Result<StatusCode, Reply>;

/// A body read as the route's own type; an empty body reads as an empty object.
pub fn body<T: DeserializeOwned>(bytes: &Bytes) -> Result<T, Reply> {
  let text: &[u8] = if bytes.iter().all(u8::is_ascii_whitespace) { b"{}" } else { bytes };

  serde_json::from_slice(text)
    .map_err(|why| Reply::bad_request(format!("the body is not the shape the route takes: {why}")))
}

/// A query read as the route's own type.
pub fn query<T: DeserializeOwned>(uri: &Uri) -> Result<T, Reply> {
  Query::try_from_uri(uri)
    .map(|Query(query)| query)
    .map_err(|why| Reply::bad_request(format!("the query is not the shape the route takes: {why}")))
}
