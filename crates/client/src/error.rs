//! The one shape every failure and refusal is answered in, and its codes, each answering the status it is
//! sent with (docs/api.md, 「Wire format」).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why a request was not answered with its route's own answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
  BadRequest,
  Unauthorized,
  NoRoute,
  NotFound,
  Conflict,
  Gone,
  NameRule,
  BadField,
  Incomplete,
  Failed,
}

impl ErrorCode {
  /// Every code, in the order the document lists them.
  pub const ALL: [Self; 10] = [
    Self::BadRequest,
    Self::Unauthorized,
    Self::NoRoute,
    Self::NotFound,
    Self::Conflict,
    Self::Gone,
    Self::NameRule,
    Self::BadField,
    Self::Incomplete,
    Self::Failed,
  ];

  /// The code as it crosses the wire.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::BadRequest => "bad-request",
      Self::Unauthorized => "unauthorized",
      Self::NoRoute => "no-route",
      Self::NotFound => "not-found",
      Self::Conflict => "conflict",
      Self::Gone => "gone",
      Self::NameRule => "name-rule",
      Self::BadField => "bad-field",
      Self::Incomplete => "incomplete",
      Self::Failed => "failed",
    }
  }

  /// The HTTP status the code is answered with.
  pub fn status(self) -> u16 {
    match self {
      Self::BadRequest => 400,
      Self::Unauthorized => 401,
      Self::NoRoute | Self::NotFound => 404,
      Self::Conflict => 409,
      Self::Gone => 410,
      Self::NameRule | Self::BadField | Self::Incomplete => 422,
      Self::Failed => 500,
    }
  }

  /// When the code is answered, as the OpenAPI document describes the answer.
  pub fn meaning(self) -> &'static str {
    match self {
      Self::BadRequest => {
        "the body or the query is not the shape the route takes; the error names what is wrong"
      }
      Self::Unauthorized => "the token is missing or wrong",
      Self::NoRoute => "no route answers this method and path",
      Self::NotFound => {
        "nothing stands at the key the path names: no node, hub or machine of that name or id"
      }
      Self::Conflict => {
        "the key is taken: a node of that name, or a hub at that address, is already listed"
      }
      Self::Gone => "an event stream resumed from before the window the service keeps",
      Self::NameRule => {
        "a node's name is not lowercase letters, digits and hyphens of at most 32 characters"
      }
      Self::BadField => {
        "a field is out of range, of the other connection type, or names nothing listed; the error names it"
      }
      Self::Incomplete => "the node's row lacks a field its connection type needs",
      Self::Failed => "the service failed; the error is the failure as a sentence",
    }
  }
}

impl fmt::Display for ErrorCode {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    out.write_str(self.as_str())
  }
}

/// The body of every answer that is not the route's own: its code and an English sentence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Error")]
pub struct WireError {
  pub code: ErrorCode,
  pub error: String,
}

impl WireError {
  pub fn new(code: ErrorCode, error: impl Into<String>) -> Self {
    Self {
      code,
      error: error.into(),
    }
  }
}

impl fmt::Display for WireError {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(out, "{} ({})", self.error, self.code)
  }
}
