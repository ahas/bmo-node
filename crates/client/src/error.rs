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
  NoNode,
  NoHub,
  NameTaken,
  HubTaken,
  NotSsh,
  Incomplete,
  NameRule,
  BadField,
  Failed,
}

impl ErrorCode {
  /// Every code, in the order the document lists them.
  pub const ALL: [Self; 12] = [
    Self::BadRequest,
    Self::Unauthorized,
    Self::NoRoute,
    Self::NoNode,
    Self::NoHub,
    Self::NameTaken,
    Self::HubTaken,
    Self::NotSsh,
    Self::Incomplete,
    Self::NameRule,
    Self::BadField,
    Self::Failed,
  ];

  /// The code as it crosses the wire.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::BadRequest => "bad-request",
      Self::Unauthorized => "unauthorized",
      Self::NoRoute => "no-route",
      Self::NoNode => "no-node",
      Self::NoHub => "no-hub",
      Self::NameTaken => "name-taken",
      Self::HubTaken => "hub-taken",
      Self::NotSsh => "not-ssh",
      Self::Incomplete => "incomplete",
      Self::NameRule => "name-rule",
      Self::BadField => "bad-field",
      Self::Failed => "failed",
    }
  }

  /// The HTTP status the code is answered with.
  pub fn status(self) -> u16 {
    match self {
      Self::BadRequest => 400,
      Self::Unauthorized => 401,
      Self::NoRoute | Self::NoNode | Self::NoHub => 404,
      Self::NameTaken | Self::HubTaken | Self::NotSsh | Self::Incomplete => 409,
      Self::NameRule | Self::BadField => 422,
      Self::Failed => 500,
    }
  }

  /// When the code is answered, as the OpenAPI document describes the answer.
  pub fn meaning(self) -> &'static str {
    match self {
      Self::BadRequest => "the body or a query is not the shape the route takes",
      Self::Unauthorized => "the token is missing or wrong",
      Self::NoRoute => "no route answers this method and path",
      Self::NoNode => "no node of that name is listed; for a machine, no machine of that name",
      Self::NoHub => "no hub of that address is listed",
      Self::NameTaken => "a node of that name is already listed",
      Self::HubTaken => "the hub is already listed",
      Self::NotSsh => "the node is reached through a hub and has no SSH route",
      Self::Incomplete => "the node lacks a field its connection type needs",
      Self::NameRule => "the name is not lowercase letters, digits and hyphens of at most 32 characters",
      Self::BadField => "a field is out of range or of the other connection type; the error names it",
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
