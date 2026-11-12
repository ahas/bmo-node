//! A script run on a jake node through its hub: the request goes to the node's own agent with the token
//! out of the hub's file, the command's lines come whole however the frames and the lines were cut, and a
//! hub's refusal is said as what it means.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hardware::hub_exec::HubExec;
use hardware::roads::{StreamEnd, stream_through};
use node_check::Machine;
use node_check::rows::Hub;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// What the stood hub was asked, and what it answers.
#[derive(Clone, Default)]
struct Asked {
  agent: Arc<Mutex<Option<String>>>,
  bearer: Arc<Mutex<Option<String>>>,
  body: Arc<Mutex<Option<Value>>>,
  status: Arc<Mutex<Option<StatusCode>>>,
  stream: Arc<Mutex<String>>,
}

fn frame(event: &str, data: Value) -> String {
  format!("event: {event}\ndata: {data}\n\n")
}

fn out(text: &str, seq: i64) -> String {
  frame("stdout", json!({ "seq": seq, "chunk": { "base64": STANDARD.encode(text) } }))
}

async fn exec(
  State(asked): State<Asked>,
  Query(query): Query<std::collections::HashMap<String, String>>,
  headers: HeaderMap,
  body: axum::Json<Value>,
) -> Response {
  *asked.agent.lock().unwrap() = query.get("agent").cloned();
  *asked.bearer.lock().unwrap() =
    headers.get("authorization").and_then(|value| value.to_str().ok()).map(str::to_owned);
  *asked.body.lock().unwrap() = Some(body.0);

  if let Some(status) = *asked.status.lock().unwrap() {
    return status.into_response();
  }

  let stream = asked.stream.lock().unwrap().clone();

  ([("content-type", "text/event-stream")], Body::from(stream)).into_response()
}

/// A hub answering its exec as the stream it is handed, and the road to its agent `agent_a`.
async fn stood(stream: String) -> (Asked, HubExec, tempfile::TempDir) {
  let asked = Asked::default();

  *asked.stream.lock().unwrap() = stream;

  let app = Router::new().route("/exec", post(exec)).with_state(asked.clone());
  let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.unwrap();
  let url = format!("http://{}", listener.local_addr().unwrap());
  let home = tempfile::tempdir().unwrap();

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
  std::fs::write(home.path().join("hub.env"), "JAKE_CONTROL_TOKEN=tok\n").unwrap();

  let road = HubExec {
    hub: Hub {
      url,
      token_file: "~/hub.env".into(),
    },
    agent: "agent_a".into(),
    powershell: false,
    machine: Machine::new("ssh", home.path()),
  };

  (asked, road, home)
}

async fn lines_of(road: &HubExec) -> (Vec<String>, StreamEnd) {
  let (lines, mut read) = mpsc::channel(64);
  let end = stream_through(road, "run", lines, CancellationToken::new()).await;
  let mut got = Vec::new();

  while let Ok(line) = read.try_recv() {
    got.push(line);
  }

  (got, end)
}

#[tokio::test]
async fn hands_on_the_command_s_lines_whole_however_the_frames_and_the_lines_were_cut() {
  let stream = frame("started", json!({ "id": "c1" }))
    + &out("{\"e\":\"st", 0)
    + &out("ep\"}\n{\"e\":\"do", 1)
    + &out("ne\"}\n", 2)
    + &frame("exit", json!({ "code": 0 }));
  let (asked, road, _home) = stood(stream).await;
  let (lines, end) = lines_of(&road).await;

  assert_eq!(lines, ["{\"e\":\"step\"}", "{\"e\":\"done\"}"]);
  assert!(end.ok);
  assert_eq!(asked.agent.lock().unwrap().as_deref(), Some("agent_a"));
  assert_eq!(asked.bearer.lock().unwrap().as_deref(), Some("Bearer tok"));
  assert_eq!(
    asked.body.lock().unwrap().clone(),
    Some(json!({ "shell": "linux", "command": "run", "env": {} }))
  );
}

#[tokio::test]
async fn a_command_s_exit_and_a_hub_s_refusal_are_said_as_what_they_mean() {
  let stream =
    frame("started", json!({ "id": "c9" })) + &out("one\n", 0) + &frame("exit", json!({ "code": 3 }));
  let (asked, road, _home) = stood(stream).await;
  let (lines, end) = lines_of(&road).await;

  assert_eq!(lines, ["one"]);
  assert_eq!(
    end,
    StreamEnd {
      ok: false,
      failure: "the script ended with 3".into()
    }
  );

  *asked.status.lock().unwrap() = Some(StatusCode::CONFLICT);
  assert_eq!(lines_of(&road).await.1.failure, "the agent agent_a is not attached to its hub");

  *asked.status.lock().unwrap() = Some(StatusCode::UNAUTHORIZED);
  assert_eq!(lines_of(&road).await.1.failure, "the hub refused the token (401)");
}

// A Windows host's agent takes the PowerShell probe packed as a short PowerShell command, so the command the agent
// starts stays inside what Windows takes; a Linux host's takes the script as it is (docs/hardware.md, 「What is
// read」).
#[test]
fn a_windows_agent_is_handed_the_probe_packed_as_a_powershell_command() {
  let hub = Hub {
    url: "http://hub".into(),
    token_file: "~/hub.env".into(),
  };
  let script = include_str!("../../crates/hardware/probe-files/live-powershell-240.ps1");
  let windows = HubExec::new(hub.clone(), "agent_w".into(), true).request(script);
  let linux = HubExec::new(hub, "agent_l".into(), false).request("echo hi");
  let command = windows["command"].as_str().unwrap();

  assert_eq!(windows["shell"], "powershell");
  assert!(command.contains("GZipStream") && !command.contains("Get-CimInstance"), "{command}");
  // The agent encodes the command as UTF-16 base64, about 2.7 times its length; it must stay under 32,767.
  assert!(command.len() * 8 / 3 + 1_000 < 32_767, "{} characters", command.len());
  assert_eq!(linux, serde_json::json!({ "shell": "linux", "command": "echo hi", "env": {} }));
}
