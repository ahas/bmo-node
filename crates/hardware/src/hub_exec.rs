//! A script run on a jake node through its hub (docs/hardware.md, 「Which machines」): the hub's exec aimed
//! at the node's own agent, its output handed on as it comes and its ending told apart. The control token
//! is read out of the hub's token file for the one request that needs it and is part of no answer.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use node_check::Machine;
use node_check::hub::hub_token;
use node_check::rows::Hub;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};
use sse_stream::SseStream;
use tokio::sync::{mpsc, oneshot};

/// How a script run through the hub ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
  /// It ran and ended with this code, and the last thing it said on its standard error.
  Code(i32, String),
  /// The road itself failed, saying why.
  Failed(String),
}

/// A script under way: its output as it comes, and its ending once it has ended.
pub struct Running {
  pub output: mpsc::Receiver<Vec<u8>>,
  pub ended: oneshot::Receiver<Ended>,
}

// How many chunks of a script's output wait for whoever reads them before the script is held.
const WAITING_CHUNKS: usize = 8;

// The most a road's failure says.
const DETAIL_MOST: usize = 200;

fn last_line(text: &str) -> String {
  text
    .lines()
    .map(str::trim)
    .rfind(|line| !line.is_empty())
    .map(|line| line.chars().take(DETAIL_MOST).collect())
    .unwrap_or_default()
}

fn encoded(value: &str) -> String {
  utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

/// The JSON an event carries, where it carries any.
fn data(event: &sse_stream::Sse) -> Value {
  event
    .data
    .as_deref()
    .and_then(|data| serde_json::from_str(data).ok())
    .unwrap_or(Value::Null)
}

fn said(body: &Value, field: &str, otherwise: &str) -> String {
  body.get(field).and_then(Value::as_str).unwrap_or(otherwise).to_owned()
}

/// A jake node's road: its hub's exec aimed at its own agent, the token read on the machine given.
#[derive(Clone)]
pub struct HubExec {
  pub hub: Hub,
  pub agent: String,
  pub machine: Machine,
}

impl HubExec {
  pub fn new(hub: Hub, agent: String) -> Self {
    Self {
      hub,
      agent,
      machine: Machine::local(),
    }
  }

  /// What a hub's answer status says where it is not the stream asked for.
  pub fn refused_status(&self, status: u16) -> Option<String> {
    match status {
      200 => None,
      401 | 403 => Some(format!("the hub refused the token ({status})")),
      404 | 409 => Some(format!("the agent {} is not attached to its hub", self.agent)),
      other => Some(format!("the hub answered {other}")),
    }
  }

  /// Starts a script in the agent's login bash.
  pub fn run(&self, script: String) -> Running {
    let (output, waiting) = mpsc::channel(WAITING_CHUNKS);
    let (ending, ended) = oneshot::channel();
    let road = self.clone();

    tokio::spawn(async move {
      let _ = ending.send(road.exec(script, output).await);
    });

    Running {
      output: waiting,
      ended,
    }
  }

  async fn exec(&self, script: String, output: mpsc::Sender<Vec<u8>>) -> Ended {
    let token = match hub_token(&self.machine, &self.hub.token_file) {
      Ok(token) => token,
      Err(why) => return Ended::Failed(why),
    };
    let asked = self
      .machine
      .http()
      .post(format!("{}/exec?agent={}", self.hub.url, encoded(&self.agent)))
      .bearer_auth(&token)
      .json(&json!({ "shell": "linux", "command": script, "env": {} }))
      .send()
      .await;
    let answer = match asked {
      Ok(answer) => answer,
      Err(why) => return Ended::Failed(format!("the hub did not answer: {}", last_line(&why.to_string()))),
    };

    if let Some(why) = self.refused_status(answer.status().as_u16()) {
      return Ended::Failed(why);
    }

    let mut events = SseStream::from_bytes_stream(answer.bytes_stream());
    let mut id: Option<String> = None;
    let mut stderr = String::new();

    while let Some(event) = events.next().await {
      let Ok(event) = event else {
        return Ended::Failed("the hub's stream broke off".into());
      };
      let body = data(&event);

      match event.event.as_deref() {
        Some("started") => id = body.get("id").and_then(Value::as_str).map(str::to_owned),
        Some("stdout" | "stderr") => {
          let Some(bytes) = body
            .pointer("/chunk/base64")
            .and_then(Value::as_str)
            .and_then(|chunk| STANDARD.decode(chunk).ok())
          else {
            continue;
          };

          if event.event.as_deref() == Some("stderr") {
            stderr.push_str(&String::from_utf8_lossy(&bytes));
          } else if output.send(bytes).await.is_err() {
            if let Some(id) = &id {
              self.cancel(&token, id).await;
            }

            return Ended::Failed("nobody listened".into());
          }
        }
        Some("exit") => {
          let code = body.get("code").and_then(Value::as_i64).and_then(|code| i32::try_from(code).ok());

          return Ended::Code(code.unwrap_or(-1), last_line(&stderr));
        }
        Some("error") => return Ended::Failed(said(&body, "message", "the hub reported an error")),
        Some("ended") => {
          return Ended::Failed(format!("the run ended: {}", said(&body, "reason", "no reason given")));
        }
        _ => {}
      }
    }

    Ended::Failed("the hub's stream closed without the command's end".into())
  }

  async fn cancel(&self, token: &str, id: &str) {
    let _ = self
      .machine
      .http()
      .post(format!("{}/cancel", self.hub.url))
      .bearer_auth(token)
      .json(&json!({ "id": id }))
      .send()
      .await;
  }
}
