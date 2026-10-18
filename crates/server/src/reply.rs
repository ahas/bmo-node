//! How a route answers (docs/api.md, 「Conventions」): its own object, a refusal as an answer, or the error
//! shape for a request that could not be answered at all.

use axum::Json;
use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use store::{Refusal, StoreError};

/// What a route answers where it does not answer its own object.
#[derive(Debug)]
pub enum Reply {
  /// The request is one the service does not take: 200, `answered` false, its code and message.
  Refused(Refusal),
  /// The request could not be answered at all: the status, its code and an English sentence.
  Failed(StatusCode, &'static str, String),
}

impl Reply {
  pub fn refused(code: &'static str, message: impl Into<String>) -> Self {
    Self::Refused(Refusal::new(code, message))
  }

  pub fn bad_request(error: impl Into<String>) -> Self {
    Self::Failed(StatusCode::BAD_REQUEST, "bad-request", error.into())
  }

  pub fn failed(error: impl Into<String>) -> Self {
    Self::Failed(StatusCode::INTERNAL_SERVER_ERROR, "failed", error.into())
  }

  pub fn no_node(name: &str) -> Self {
    Self::refused("no-node", format!("no node named {name}"))
  }
}

impl IntoResponse for Reply {
  fn into_response(self) -> Response {
    match self {
      Self::Refused(refusal) => Json(json!({
        "answered": false,
        "refusal": { "code": refusal.code, "message": refusal.message },
      }))
      .into_response(),
      Self::Failed(status, code, error) => {
        (status, Json(json!({ "code": code, "error": error }))).into_response()
      }
    }
  }
}

impl From<StoreError> for Reply {
  fn from(error: StoreError) -> Self {
    match error {
      StoreError::Refused(refusal) => Self::Refused(refusal),
      other => Self::failed(other.to_string()),
    }
  }
}

/// A route's answer: its own object, or a reply of the other kinds.
pub type Answer = Result<Json<Value>, Reply>;

/// A body read as the route's own shape; an empty body reads as an empty object.
pub fn body<T: DeserializeOwned>(bytes: &Bytes) -> Result<T, Reply> {
  let text: &[u8] = if bytes.iter().all(u8::is_ascii_whitespace) { b"{}" } else { bytes };

  serde_json::from_slice(text)
    .map_err(|why| Reply::bad_request(format!("the body is not the shape the route takes: {why}")))
}
