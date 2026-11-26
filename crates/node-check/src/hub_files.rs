//! One file's bytes moved on a jake node through its hub (docs/api.md, 「File transfer」): the hub's
//! `POST /upload` and `POST /download` aimed at the node's own agent, with the control token read out of the
//! hub's token file for that one request. The bytes stream through and are held by neither side: an upload's
//! body goes on as it arrives, and a download's chunks come on as the agent reads them. The agent moves an
//! upload into place only once its bytes match the declared total and checksum, and the hub ends a download
//! only once what it assembled matches what the agent declared; neither check is made twice here, and
//! nothing of a file or of the token is part of any answer.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::StreamExt;
use futures::stream::{BoxStream, unfold};
use serde_json::Value;
use sse_stream::{Sse, SseStream};

use crate::hub::hub_token;
use crate::hub_exec::{data, encoded, refused};
use crate::rows::Hub;
use crate::{Machine, last_line};

/// Why a transfer did not go, and in whose way it lay, as the hub and its agent told it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
  /// The path is not one the node takes: the folder it stands in is not there, or the node may not write or
  /// read where it names. The line the hub or the agent said.
  BadPath(String),
  /// Nothing stands at the path a download named. The line the agent said.
  NoFile(String),
  /// What the transfer needed did not answer, or its bytes did not come whole: the hub refusing the token,
  /// the agent gone, the hub silent, the token file unreadable, or bytes that did not verify.
  Hub(String),
}

impl Failed {
  fn hub(why: impl Into<String>) -> Self {
    Self::Hub(why.into())
  }

  /// The silent the hub did not answer.
  fn silent(why: &str) -> Self {
    Self::hub(format!("the hub did not answer: {}", last_line(why).unwrap_or_default()))
  }

  /// What the hub answered with where it is not the stream asked for: the caller's own path where the hub
  /// took the request's shape, the transfer's failure everywhere else.
  fn answered(status: u16, agent: &str, said: &Value) -> Self {
    let why = refused(status, agent, said).unwrap_or_default();

    match status {
      400 => Self::BadPath(why),
      _ => Self::hub(why),
    }
  }

  /// The code and the line of an error frame the hub or its agent answered a transfer with. The code is
  /// jake's: `not-found` for a path with nothing at it, `permission` and `is-directory` for one its account
  /// may not write or read, `integrity` for bytes that did not verify.
  fn fault(said: &Value) -> (&str, String) {
    let code = said.get("code").and_then(Value::as_str).unwrap_or_default();
    let message = said.get("message").and_then(Value::as_str).unwrap_or("the transfer was refused");
    let why = if code.is_empty() { message.to_owned() } else { format!("{code}: {message}") };

    (code, why)
  }

  /// An upload the hub or its agent refused: a path the caller named that the node will not take, or the
  /// transfer's own failure, bytes that did not verify among them.
  fn upload_frame(said: &Value) -> Self {
    let (code, why) = Self::fault(said);

    match code {
      "not-found" | "permission" | "is-directory" => Self::BadPath(why),
      _ => Self::hub(why),
    }
  }

  /// A download the hub or its agent refused: nothing standing at the path the caller named, a path its
  /// account may not read, or the transfer's own failure.
  fn download_frame(said: &Value) -> Self {
    let (code, why) = Self::fault(said);

    match code {
      "not-found" => Self::NoFile(why),
      "permission" | "is-directory" => Self::BadPath(why),
      _ => Self::hub(why),
    }
  }

  /// A transfer the hub ended of its own accord, the agent's socket being gone or never there.
  fn ended(said: &Value) -> Self {
    Self::hub(format!(
      "the hub ended the transfer: {}",
      said.get("reason").and_then(Value::as_str).unwrap_or("no reason given")
    ))
  }
}

/// A file a download is bringing: its byte count as the agent declared it, and its bytes as they come, the
/// stream ending when the hub says the file is whole.
pub struct Download {
  pub total: u64,
  pub bytes: BoxStream<'static, Vec<u8>>,
}

/// The hub's transfer stream, read the same way for either verb.
fn stream_of(answer: reqwest::Response) -> BoxStream<'static, Option<Sse>> {
  SseStream::from_bytes_stream(answer.bytes_stream()).map(|event| event.ok()).boxed()
}

/// Writes `body` at `path` on `agent` of `hub`, the whole file declared to be `total` bytes hashing to
/// `checksum`. Ok once the agent has acknowledged the verified write; the body is not read past that.
pub async fn upload(
  machine: &Machine,
  hub: &Hub,
  agent: &str,
  path: &str,
  total: u64,
  checksum: &str,
  body: reqwest::Body,
) -> Result<(), Failed> {
  let token = hub_token(machine, &hub.token_file).map_err(Failed::hub)?;
  let url = format!(
    "{}/upload?agent={}&path={}&total={}&checksum={}",
    hub.url,
    encoded(agent),
    encoded(path),
    total,
    encoded(checksum)
  );
  let answer = machine
    .http()
    .post(url)
    .bearer_auth(&token)
    .body(body)
    .send()
    .await
    .map_err(|why| Failed::silent(&why.to_string()))?;
  let status = answer.status().as_u16();

  if status != 200 {
    let said = answer.json::<Value>().await.unwrap_or(Value::Null);

    return Err(Failed::answered(status, agent, &said));
  }

  let mut events = stream_of(answer);

  while let Some(event) = events.next().await {
    let Some(event) = event else {
      return Err(Failed::hub("the hub's stream broke off"));
    };
    let said = data(&event);

    match event.event.as_deref() {
      Some("completed") => return Ok(()),
      Some("error") => return Err(Failed::upload_frame(&said)),
      Some("ended") => return Err(Failed::ended(&said)),
      _ => {}
    }
  }

  Err(Failed::hub("the hub's stream closed without the transfer's end"))
}

/// Asks `agent` of `hub` for the file at `path`, and brings back its bytes. The answer says whether the file
/// is there before any of its bytes are handed on.
pub async fn download(machine: &Machine, hub: &Hub, agent: &str, path: &str) -> Result<Download, Failed> {
  let token = hub_token(machine, &hub.token_file).map_err(Failed::hub)?;
  let url = format!("{}/download?agent={}&path={}", hub.url, encoded(agent), encoded(path));
  let answer = machine
    .http()
    .post(url)
    .bearer_auth(&token)
    .send()
    .await
    .map_err(|why| Failed::silent(&why.to_string()))?;
  let status = answer.status().as_u16();

  if status != 200 {
    let said = answer.json::<Value>().await.unwrap_or(Value::Null);

    return Err(Failed::answered(status, agent, &said));
  }

  let mut events = stream_of(answer);

  while let Some(event) = events.next().await {
    let Some(event) = event else {
      return Err(Failed::hub("the hub's stream broke off"));
    };
    let said = data(&event);

    match event.event.as_deref() {
      Some("begin") => {
        let Some(total) = said.get("total").and_then(Value::as_u64) else {
          return Err(Failed::hub("the hub began a download declaring no total"));
        };

        return Ok(Download {
          total,
          bytes: chunks(events),
        });
      }
      Some("error") => return Err(Failed::download_frame(&said)),
      Some("ended") => return Err(Failed::ended(&said)),
      _ => {}
    }
  }

  Err(Failed::hub("the hub's stream closed without the file's beginning"))
}

/// The chunk frames of a download, decoded to bytes, ending when the hub says the file is whole. A transfer
/// that failed after its bytes had started ends the stream short of its declared length: the caller of the
/// service sees the body break off, and nothing here can still answer for it.
fn chunks(events: BoxStream<'static, Option<Sse>>) -> BoxStream<'static, Vec<u8>> {
  unfold(events, move |mut events| async move {
    while let Some(event) = events.next().await {
      // An SSE item that broke off ends the body where it stands.
      let event = event?;
      let said = data(&event);

      match event.event.as_deref() {
        Some("chunk") => {
          let Some(bytes) = said
            .pointer("/chunk/base64")
            .and_then(Value::as_str)
            .and_then(|piece| STANDARD.decode(piece).ok())
          else {
            continue;
          };

          return Some((bytes, events));
        }
        Some("completed" | "error" | "ended") => return None,
        _ => {}
      }
    }

    None
  })
  .boxed()
}
