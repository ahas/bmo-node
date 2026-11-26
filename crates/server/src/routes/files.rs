//! A file transfer (docs/api.md, 「File transfer」): one file's bytes moved to or from a hub node through its
//! hub, for a person whose own node it is, under exactly the judgement a run already gets. The bytes stream
//! through: an upload's body goes on to the hub as it arrives and is never held here, and a download's
//! chunks go on to the caller as the hub reads them off the agent. Nothing of a file is kept, and no record
//! of a transfer is made.

use axum::body::Body;
use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use bmo_node_client::files::{DownloadQuery, Upload, UploadQuery};
use futures_util::StreamExt;
use node_check::hub_files::{self, Failed};

use super::{Caller, Doing, Reached, Shared, let_through, upstream};
use crate::reply::{Answer, Reply, query};

/// Where a transfer's failure lay, in the answer docs/api.md gives it (「File transfer」).
fn refused(node: &str, path: &str, why: Failed) -> Reply {
  match why {
    Failed::BadPath(said) => {
      Reply::bad_request(format!("The node {node} will not take the path {path}: {said}."))
    }
    Failed::NoFile(said) => Reply::not_found(format!("The node {node} has no file at {path}: {said}.")),
    Failed::Hub(said) => upstream(format!("The file transfer on {node} did not go: {said}.")),
  }
}

/// The file's raw bytes, declared whole by the query, written on the node through its hub.
pub async fn upload(
  service: Shared,
  Path(name): Path<String>,
  Extension(Caller(caller)): Extension<Caller>,
  uri: Uri,
  body: Body,
) -> Answer<Upload> {
  let asked: UploadQuery = query(&uri)?;
  let Reached { hub, agent, .. } =
    let_through(&service, &name, &caller, &asked.account, Doing::File).await?;

  // The body is handed to the hub as it arrives rather than read first, so no size limit stands between a
  // caller and a file of any size (docs/api.md, 「File transfer」).
  let bytes = reqwest::Body::wrap_stream(body.into_data_stream());

  hub_files::upload(&service.machine, &hub, &agent, &asked.path, asked.total, &asked.checksum, bytes)
    .await
    .map_err(|why| refused(&name, &asked.path, why))?;

  Ok(Json(Upload {
    node: name,
    path: asked.path,
    total: asked.total,
    checksum: asked.checksum,
  }))
}

/// The file's raw bytes, streamed from the node through its hub, at the length the agent declared.
pub async fn download(
  service: Shared,
  Path(name): Path<String>,
  Extension(Caller(caller)): Extension<Caller>,
  uri: Uri,
) -> Result<Response, Reply> {
  let asked: DownloadQuery = query(&uri)?;
  let Reached { hub, agent, .. } =
    let_through(&service, &name, &caller, &asked.account, Doing::File).await?;

  // Whether the file is there is known before any byte is handed on, so every refusal is still an answer in
  // the error shape. Once the bytes have started an answer cannot change: a transfer that breaks after that
  // leaves the caller with a body short of the length declared here.
  let coming = hub_files::download(&service.machine, &hub, &agent, &asked.path)
    .await
    .map_err(|why| refused(&name, &asked.path, why))?;
  let mut headers = HeaderMap::new();

  headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
  headers.insert(
    header::CONTENT_LENGTH,
    HeaderValue::from_str(&coming.total.to_string()).expect("a byte count is a header value"),
  );

  Ok((headers, Body::from_stream(coming.bytes.map(Ok::<Vec<u8>, std::io::Error>))).into_response())
}
