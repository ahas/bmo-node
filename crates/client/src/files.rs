//! A file transfer (docs/api.md, 「File transfer」): one file's bytes moved to or from a hub node for a
//! person whose own node it is, the node's hub's own upload and download relayed. The query declares the
//! whole file; the service holds nothing of it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What an upload asks beside the file's bytes: whose node it is, where the file goes, and what its whole
/// is, as the hub's agent checks the bytes it assembled against them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadQuery {
  /// The person whose node it must be, as the account's id written out.
  pub account: String,
  /// The whole file's path on the node.
  pub path: String,
  /// The body's byte count.
  pub total: u64,
  /// The body's sha256, in lowercase hex.
  pub checksum: String,
}

/// What a download asks: whose node it is, and which file to read back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadQuery {
  /// The person whose node it must be, as the account's id written out.
  pub account: String,
  /// The whole file's path on the node.
  pub path: String,
}

/// A file an upload wrote on the node, as the node's hub's agent acknowledged it: written only once its
/// bytes matched what the query declared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Upload {
  /// The node it was written on.
  pub node: String,
  /// The path it was written at.
  pub path: String,
  /// Its byte count.
  pub total: u64,
  /// Its sha256, in lowercase hex.
  pub checksum: String,
}
