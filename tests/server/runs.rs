//! A run (docs/api.md, 「Runs」) against a stand-in of the hub and the owners' read on one port: a caller the
//! node lets on runs a command for the person whose node it is, and a caller the node does not let on, a
//! person whose node it is not and a node the service does not hold are refused before the hub is asked.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use node_server::config::Caller;
use node_server::owners::OwnersRead;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::support::{Stand, Stood, TOKEN};

const HUB_TOKEN: &str = "hub-control-token";
const OWNERS_TOKEN: &str = "owners-read-token";
const GRANTED: (&str, &str) = ("desk-driver", "desk-driver-token");
const OTHER: (&str, &str) = ("other-driver", "other-driver-token");
const AGENT: &str = "pc-four";

/// One exec the hub was asked: its agent, its bearer and its body.
type Exec = (Option<String>, Option<String>, Value);

/// One owners' read: the node it named and its bearer.
type Read = (String, Option<String>);

/// What the stand-in was asked: each exec, and each owners' read.
#[derive(Clone, Default)]
struct Asked {
  execs: Arc<Mutex<Vec<Exec>>>,
  reads: Arc<Mutex<Vec<Read>>>,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
  headers.get("authorization").and_then(|value| value.to_str().ok()).map(str::to_owned)
}

fn frame(event: &str, data: Value) -> String {
  format!("event: {event}\ndata: {data}\n\n")
}

/// The hub's exec: the command started, said `hello` and ended 0.
async fn exec(
  State(asked): State<Asked>,
  Query(query): Query<HashMap<String, String>>,
  headers: HeaderMap,
  body: axum::Json<Value>,
) -> Response {
  asked
    .execs
    .lock()
    .unwrap()
    .push((query.get("agent").cloned(), bearer(&headers), body.0));

  let stream = [
    frame("started", json!({ "id": "run-1" })),
    frame("stdout", json!({ "seq": 1, "chunk": { "base64": STANDARD.encode(b"hello\n") } })),
    frame("exit", json!({ "code": 0 })),
  ]
  .concat();

  ([("content-type", "text/event-stream")], Body::from(stream)).into_response()
}

/// The owners' read: the node is account 4's.
async fn access(State(asked): State<Asked>, Path(node): Path<String>, headers: HeaderMap) -> Response {
  asked.reads.lock().unwrap().push((node.clone(), bearer(&headers)));
  axum::Json(json!({ "node": node, "owners": [4] })).into_response()
}

struct Ran {
  stood: Stood,
  asked: Asked,
  node: String,
  /// The owners' token file, read for each run, so it stands as long as the service does.
  _keys: tempfile::TempDir,
}

/// The service with two callers and the owners' read, and the hub node `pc-four` letting the first caller on.
async fn stand() -> Ran {
  let asked = Asked::default();
  let app = Router::new()
    .route("/exec", post(exec))
    .route("/access/{node}", get(access))
    .with_state(asked.clone());
  let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.unwrap();
  let url = format!("http://{}", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

  let keys = tempfile::tempdir().unwrap();
  let owners_token = keys.path().join("owners.token");

  std::fs::write(&owners_token, format!("{OWNERS_TOKEN}\n")).unwrap();

  let caller = |(name, token): (&str, &str)| Caller {
    name: name.into(),
    token: token.into(),
  };
  let stood = Stand {
    callers: vec![caller(GRANTED), caller(OTHER)],
    owners: Some(OwnersRead {
      url: format!("{url}/access/{{nodeName}}?agent={{caller}}"),
      token_file: owners_token,
    }),
    ..Stand::default()
  }
  .up()
  .await;

  std::fs::write(stood.dir.path().join("hub.env"), format!("JAKE_CONTROL_TOKEN={HUB_TOKEN}\n")).unwrap();

  let store = &stood.service.store;

  store.add_hub(&url, "~/hub.env", None).unwrap();
  store.add_agent(&url, AGENT).unwrap();
  store.set_callers(AGENT, Some(GRANTED.0.into())).unwrap();

  Ran {
    stood,
    asked,
    node: AGENT.into(),
    _keys: keys,
  }
}

impl Ran {
  async fn run(&self, token: &str, node: &str, account: &str) -> (StatusCode, Value) {
    let body = json!({ "account": account, "command": "echo hello" });

    self
      .stood
      .ask_with(Some(token), Method::POST, &format!("/nodes/{node}/runs"), Some(body))
      .await
  }
}

#[tokio::test]
async fn a_caller_the_node_lets_on_runs_a_command_for_the_person_whose_node_it_is() {
  let ran = stand().await;
  let (status, body) = ran.run(GRANTED.1, &ran.node, "4").await;

  assert_eq!(status, StatusCode::OK, "{body}");
  assert_eq!(
    body,
    json!({
      "node": AGENT, "shell": "linux", "exitCode": 0, "signal": null, "stdout": "hello\n", "stderr": "",
      "stdoutCut": false, "stderrCut": false,
    })
  );
  assert_eq!(
    ran.asked.reads.lock().unwrap().clone(),
    [(AGENT.to_owned(), Some(format!("Bearer {OWNERS_TOKEN}")))]
  );

  let execs = ran.asked.execs.lock().unwrap().clone();

  assert_eq!(execs.len(), 1);
  assert_eq!(execs[0].0.as_deref(), Some(AGENT));
  assert_eq!(execs[0].1, Some(format!("Bearer {HUB_TOKEN}")));
  assert_eq!(execs[0].2["command"], "echo hello");
  assert!(!body.to_string().contains(HUB_TOKEN));
}

#[tokio::test]
async fn a_caller_the_node_does_not_let_on_and_a_person_whose_node_it_is_not_are_refused() {
  let ran = stand().await;
  let (status, body) = ran.run(OTHER.1, &ran.node, "4").await;

  assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
  assert_eq!(body["code"], "forbidden");
  assert!(body["error"].as_str().unwrap().contains(OTHER.0), "{body}");
  assert!(ran.asked.reads.lock().unwrap().is_empty());

  let (status, body) = ran.run(GRANTED.1, &ran.node, "5").await;

  assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
  assert!(body["error"].as_str().unwrap().contains("account 4's, not account 5's"), "{body}");
  assert!(ran.asked.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_run_takes_a_caller_s_own_token_and_a_caller_s_token_takes_no_other_route() {
  let ran = stand().await;
  let (status, body) = ran.run(TOKEN, &ran.node, "4").await;

  assert_eq!((status, body["code"].clone()), (StatusCode::FORBIDDEN, json!("forbidden")), "{body}");

  let (status, body) = ran.run("no-such-token", &ran.node, "4").await;

  assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

  let (status, body) = ran.stood.ask_with(Some(GRANTED.1), Method::GET, "/nodes", None).await;

  assert_eq!((status, body["code"].clone()), (StatusCode::FORBIDDEN, json!("forbidden")), "{body}");
  assert!(ran.asked.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_the_service_does_not_hold_is_not_found() {
  let ran = stand().await;
  let (status, body) = ran.run(GRANTED.1, "no-such-node", "4").await;

  assert_eq!((status, body["code"].clone()), (StatusCode::NOT_FOUND, json!("not-found")), "{body}");
  assert!(ran.asked.reads.lock().unwrap().is_empty());
  assert!(ran.asked.execs.lock().unwrap().is_empty());
}
