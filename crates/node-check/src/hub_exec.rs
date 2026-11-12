//! A command run on a jake node through its hub (docs/nodes.md, 「Reaching a node」): the hub's
//! `POST /exec` aimed at the node's own agent, with the control token read out of the hub's token file for
//! that one request, and the hub's stream read into what the command said and how it ended. The token is
//! part of no answer and no message.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::StreamExt;
use futures::stream::BoxStream;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde_json::{Value, json};
use sse_stream::{Sse, SseStream};

use crate::hub::hub_token;
use crate::rows::Hub;
use crate::{Machine, last_line};

/// What the hub's exec is handed: the shell, the command, and where and with what variables it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecBody {
  /// `powershell`, `wsl` or `linux`.
  pub shell: String,
  pub command: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cwd: Option<String>,
  pub env: BTreeMap<String, String>,
  /// The WSL distribution a `wsl` command runs in.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub distro: Option<String>,
}

impl ExecBody {
  /// A command in `shell`, run where the agent starts it, with no variables of its own.
  pub fn plain(shell: &str, command: impl Into<String>) -> Self {
    Self {
      shell: shell.to_owned(),
      command: command.into(),
      cwd: None,
      env: BTreeMap::new(),
      distro: None,
    }
  }
}

/// One thing the command said on its way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
  /// The hub took the command under this id.
  Started(String),
  Stdout(Vec<u8>),
  Stderr(Vec<u8>),
  /// The command ended: its code, and the signal that ended it, where the hub names either.
  Exit {
    code: Option<i64>,
    signal: Option<String>,
  },
}

/// A command under way on an agent, read one thing at a time.
pub struct HubRun {
  events: BoxStream<'static, Option<Sse>>,
  machine: Machine,
  url: String,
  token: String,
  id: Option<String>,
}

fn encoded(value: &str) -> String {
  utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

/// The JSON an event carries, where it carries any.
fn data(event: &Sse) -> Value {
  event
    .data
    .as_deref()
    .and_then(|data| serde_json::from_str(data).ok())
    .unwrap_or(Value::Null)
}

fn said(body: &Value, field: &str, otherwise: &str) -> String {
  body.get(field).and_then(Value::as_str).unwrap_or(otherwise).to_owned()
}

/// What a hub's answer status says where it is not the stream asked for, with the hub's own sentence where
/// it gave one.
pub fn refused(status: u16, agent: &str, body: &Value) -> Option<String> {
  match status {
    200 => None,
    401 | 403 => Some(format!("the hub refused the token ({status})")),
    404 | 409 => Some(format!("the agent {agent} is not attached to its hub")),
    other => Some(match body.get("error").and_then(Value::as_str) {
      Some(error) => format!("the hub answered {other}: {}", last_line(error).unwrap_or_default()),
      None => format!("the hub answered {other}"),
    }),
  }
}

/// Starts `body` on the agent `agent` of `hub`, or says why it could not be started.
pub async fn start(machine: &Machine, hub: &Hub, agent: &str, body: &ExecBody) -> Result<HubRun, String> {
  let token = hub_token(machine, &hub.token_file)?;
  let asked = machine
    .http()
    .post(format!("{}/exec?agent={}", hub.url, encoded(agent)))
    .bearer_auth(&token)
    .json(body)
    .send()
    .await;
  let answer = asked.map_err(|why| {
    format!("the hub did not answer: {}", last_line(&why.to_string()).unwrap_or_default())
  })?;
  let status = answer.status().as_u16();

  if status != 200 {
    let body = answer.json::<Value>().await.unwrap_or(Value::Null);

    return Err(refused(status, agent, &body).unwrap_or_default());
  }

  Ok(HubRun {
    events: SseStream::from_bytes_stream(answer.bytes_stream()).map(|event| event.ok()).boxed(),
    machine: machine.clone(),
    url: hub.url.clone(),
    token,
    id: None,
  })
}

impl HubRun {
  /// The next thing the command said, or why the run failed before its end: the hub's own error, a run the
  /// hub ended, or a stream that broke off or closed without the command's end.
  pub async fn next(&mut self) -> Result<Said, String> {
    while let Some(event) = self.events.next().await {
      let Some(event) = event else {
        return Err("the hub's stream broke off".into());
      };
      let body = data(&event);

      match event.event.as_deref() {
        Some("started") => {
          let id = said(&body, "id", "");

          self.id = Some(id.clone());
          return Ok(Said::Started(id));
        }
        Some(kind @ ("stdout" | "stderr")) => {
          let Some(bytes) = body
            .pointer("/chunk/base64")
            .and_then(Value::as_str)
            .and_then(|chunk| STANDARD.decode(chunk).ok())
          else {
            continue;
          };

          return Ok(if kind == "stdout" { Said::Stdout(bytes) } else { Said::Stderr(bytes) });
        }
        Some("exit") => {
          return Ok(Said::Exit {
            code: body.get("code").and_then(Value::as_i64),
            signal: body.get("signal").and_then(Value::as_str).map(str::to_owned),
          });
        }
        Some("error") => return Err(said(&body, "message", "the hub reported an error")),
        Some("ended") => {
          return Err(format!("the run ended: {}", said(&body, "reason", "no reason given")));
        }
        _ => {}
      }
    }

    Err("the hub's stream closed without the command's end".into())
  }

  /// Asks the hub to cancel the command, where it has said its id, and gives the run up.
  pub async fn cancel(self) {
    let Some(id) = &self.id else {
      return;
    };
    let _ = self
      .machine
      .http()
      .post(format!("{}/cancel", self.url))
      .bearer_auth(&self.token)
      .json(&json!({ "id": id }))
      .send()
      .await;
  }
}
