//! A file transfer (docs/api.md, 「File transfer」) against a stand-in of the hub and the owners' read on one
//! port: a caller the node lets writes one file's bytes whole and reads them back whole, under exactly the
//! judgement a run gets, and what the hub or its agent refuses is answered in the shape the document states.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_node_client::NodeClient;
use bmo_node_client::files::{DownloadQuery, UploadQuery};
use futures_util::TryStreamExt;
use node_server::config::Caller;
use node_server::owners::OwnersRead;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use store::nodes::SshLogin;

use super::support::{Stand, Stood, TOKEN, holds_to_contract};

const HUB_TOKEN: &str = "hub-control-token";
const OWNERS_TOKEN: &str = "owners-read-token";
const TESTER: (&str, &str) = ("tester", "tester-token");
const OTHER: (&str, &str) = ("other-tester", "other-tester-token");
const NODE: &str = "node-a";
const PATH: &str = "/srv/notes/notes.txt";
const CHECKSUM: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FILE: &[u8] = b"first line\nsecond line\n";

/// One upload the stand-in hub was asked, as it was asked.
#[derive(Clone, Debug)]
struct Put {
  agent: Option<String>,
  path: String,
  total: Option<String>,
  checksum: Option<String>,
  bearer: Option<String>,
  bytes: Vec<u8>,
}

/// One download the stand-in hub was asked.
#[derive(Clone, Debug)]
struct Take {
  agent: Option<String>,
  path: String,
  bearer: Option<String>,
}

/// What the stand-in hub answers an upload with, as the hub and its agent answer.
#[derive(Clone, Debug)]
enum PutAnswer {
  /// The agent assembled the bytes, verified them and moved the file into place.
  Written,
  /// The agent refused the transfer, naming its own code and the line it said.
  Agent { code: String, message: String },
  /// The hub refused the request itself, before taking any of the body.
  Hub { status: u16, error: String },
}

/// What the stand-in hub answers a download with.
#[derive(Clone, Debug)]
enum TakeAnswer {
  /// The agent has the file and streams it, in two chunks as a whole file is streamed.
  Has(Vec<u8>),
  /// The agent refused the transfer, naming its own code and the line it said.
  Agent { code: String, message: String },
}

/// One owners' read: the node it named and its bearer.
type Read = (String, Option<String>);

/// What the stand-in was asked, and how it answers.
#[derive(Clone)]
struct Asked {
  puts: Arc<Mutex<Vec<Put>>>,
  takes: Arc<Mutex<Vec<Take>>>,
  /// Each owners' read.
  reads: Arc<Mutex<Vec<Read>>>,
  put: Arc<Mutex<PutAnswer>>,
  take: Arc<Mutex<TakeAnswer>>,
}

impl Default for Asked {
  fn default() -> Self {
    Self {
      puts: Arc::default(),
      takes: Arc::default(),
      reads: Arc::default(),
      put: Arc::new(Mutex::new(PutAnswer::Written)),
      take: Arc::new(Mutex::new(TakeAnswer::Has(FILE.to_vec()))),
    }
  }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
  headers.get("authorization").and_then(|value| value.to_str().ok()).map(str::to_owned)
}

fn frame(event: &str, data: Value) -> String {
  format!("event: {event}\ndata: {data}\n\n")
}

fn sse(frames: String) -> Response {
  ([("content-type", "text/event-stream")], Body::from(frames)).into_response()
}

/// The stand-in hub's upload: it takes the body as the hub's own pump does and answers what the test said.
async fn upload(
  State(asked): State<Asked>,
  Query(query): Query<HashMap<String, String>>,
  headers: HeaderMap,
  body: axum::body::Bytes,
) -> Response {
  let one = Put {
    agent: query.get("agent").cloned(),
    path: query.get("path").cloned().unwrap_or_default(),
    total: query.get("total").cloned(),
    checksum: query.get("checksum").cloned(),
    bearer: bearer(&headers),
    bytes: body.to_vec(),
  };

  asked.puts.lock().unwrap().push(one.clone());

  match &*asked.put.lock().unwrap() {
    PutAnswer::Written => sse(
      [
        frame("started", json!({ "id": "u-1" })),
        frame("completed", json!({ "total": one.bytes.len(), "checksum": one.checksum })),
      ]
      .concat(),
    ),
    PutAnswer::Agent { code, message } => sse(
      [
        frame("started", json!({ "id": "u-1" })),
        frame("error", json!({ "code": code, "message": message })),
      ]
      .concat(),
    ),
    PutAnswer::Hub { status, error } => {
      (axum::http::StatusCode::from_u16(*status).unwrap(), Json(json!({ "error": error }))).into_response()
    }
  }
}

/// The stand-in hub's download: the agent's begin, chunks and end, or its refusal.
async fn download(
  State(asked): State<Asked>,
  Query(query): Query<HashMap<String, String>>,
  headers: HeaderMap,
) -> Response {
  asked.takes.lock().unwrap().push(Take {
    agent: query.get("agent").cloned(),
    path: query.get("path").cloned().unwrap_or_default(),
    bearer: bearer(&headers),
  });

  match &*asked.take.lock().unwrap() {
    TakeAnswer::Has(bytes) => {
      let (first, second) = bytes.split_at(bytes.len() / 2);
      let chunk = |seq: u64, piece: &[u8]| json!({ "seq": seq, "chunk": { "bytes": piece.len(), "base64": STANDARD.encode(piece) } });

      sse(
        [
          frame("started", json!({ "id": "d-1" })),
          frame("begin", json!({ "total": bytes.len(), "checksum": CHECKSUM })),
          frame("chunk", chunk(0, first)),
          frame("chunk", chunk(1, second)),
          frame("completed", json!({ "total": bytes.len(), "checksum": CHECKSUM })),
        ]
        .concat(),
      )
    }
    TakeAnswer::Agent { code, message } => sse(
      [
        frame("started", json!({ "id": "d-1" })),
        frame("error", json!({ "code": code, "message": message })),
      ]
      .concat(),
    ),
  }
}

/// The owners' read: the node is account 4's.
async fn access(State(asked): State<Asked>, Path(node): Path<String>, headers: HeaderMap) -> Response {
  asked.reads.lock().unwrap().push((node.clone(), bearer(&headers)));
  axum::Json(json!({ "node": node, "owners": [4] })).into_response()
}

struct Moved {
  stood: Stood,
  asked: Asked,
  /// The owners' token file, read for each transfer, so it stands as long as the service does.
  _keys: tempfile::TempDir,
}

/// The service with two callers and the owners' read, and the hub node `node-a` letting the first caller on.
async fn stand() -> Moved {
  let asked = Asked::default();
  let app = Router::new()
    .route("/upload", post(upload))
    .route("/download", post(download))
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
    callers: vec![caller(TESTER), caller(OTHER)],
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
  store.add_agent(&url, NODE).unwrap();
  store.set_callers(NODE, Some(TESTER.0.into())).unwrap();

  Moved {
    stood,
    asked,
    _keys: keys,
  }
}

impl Moved {
  /// `POST /nodes/{node}/file` with `bytes` as its body, asked with `token`.
  async fn put(
    &self,
    token: &str,
    node: &str,
    account: &str,
    path: &str,
    bytes: &[u8],
  ) -> (StatusCode, Value) {
    let answer = self
      .stood
      .client
      .post(format!("{}/nodes/{node}/file", self.stood.base))
      .bearer_auth(token)
      .query(&[
        ("account", account.to_owned()),
        ("path", path.to_owned()),
        ("total", bytes.len().to_string()),
        ("checksum", CHECKSUM.to_owned()),
      ])
      .body(bytes.to_vec())
      .send()
      .await
      .unwrap();
    let status = answer.status();
    let text = answer.text().await.unwrap();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));

    holds_to_contract(&Method::POST, &format!("/nodes/{node}/file"), status, &body);
    (status, body)
  }

  /// `GET /nodes/{node}/file`, asked with `token`: the file's bytes, or the refusal naming its path.
  async fn take(
    &self,
    token: &str,
    node: &str,
    account: &str,
    path: &str,
  ) -> (StatusCode, HeaderMap, Vec<u8>) {
    let answer = self
      .stood
      .client
      .get(format!("{}/nodes/{node}/file", self.stood.base))
      .bearer_auth(token)
      .query(&[("account", account.to_owned()), ("path", path.to_owned())])
      .send()
      .await
      .unwrap();
    let status = answer.status();
    let headers = answer.headers().clone();
    let bytes = answer.bytes().await.unwrap().to_vec();
    // The answer is the file itself where it is not a refusal; only a refusal has a JSON body to hold to.
    let said = match headers.get("content-type").and_then(|value| value.to_str().ok()) {
      Some(kind) if kind.starts_with("application/json") => {
        let text = String::from_utf8_lossy(&bytes).to_string();

        serde_json::from_str(&text).unwrap_or(Value::String(text))
      }
      _ => json!(""),
    };

    holds_to_contract(&Method::GET, &format!("/nodes/{node}/file"), status, &said);
    (status, headers, bytes)
  }

  /// The refusal an answer carries, read as the error shape it is answered in.
  fn refusal(&self, bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::String(String::from_utf8_lossy(bytes).into_owned()))
  }

  /// The upload the stand-in hub was last asked for.
  fn put_asked(&self) -> Put {
    self
      .asked
      .puts
      .lock()
      .unwrap()
      .last()
      .unwrap_or_else(|| panic!("the hub was asked for no upload"))
      .clone()
  }

  fn refused_with(&self, answer: PutAnswer) {
    *self.asked.put.lock().unwrap() = answer;
  }

  fn answered_with(&self, answer: TakeAnswer) {
    *self.asked.take.lock().unwrap() = answer;
  }
}

/// The line jake's agent says for a path whose folder is not there, and for bytes that did not verify.
const MISSING: &str = "no such folder";
const UNVERIFIED: &str = "assembled 19 bytes did not match the declared transfer";
const NO_FILE: &str = "/srv/notes/notes.txt: No such file or directory (os error 2)";

#[tokio::test]
async fn a_caller_the_node_lets_writes_one_file_whole_and_is_told_what_stands_written() {
  let moved = stand().await;
  let (status, body) = moved.put(TESTER.1, NODE, "4", PATH, FILE).await;

  assert_eq!(status, StatusCode::OK, "{body}");
  assert_eq!(body, json!({ "node": NODE, "path": PATH, "total": FILE.len(), "checksum": CHECKSUM }));
  assert_eq!(
    moved.asked.reads.lock().unwrap().clone(),
    [(NODE.to_owned(), Some(format!("Bearer {OWNERS_TOKEN}")))]
  );

  let asked = moved.put_asked();

  assert_eq!(asked.agent.as_deref(), Some(NODE));
  assert_eq!(asked.path, PATH);
  assert_eq!(asked.total.as_deref(), Some(FILE.len().to_string().as_str()));
  assert_eq!(asked.checksum.as_deref(), Some(CHECKSUM));
  assert_eq!(asked.bearer, Some(format!("Bearer {HUB_TOKEN}")));
  assert_eq!(asked.bytes, FILE);
  assert!(!body.to_string().contains(HUB_TOKEN));
}

#[tokio::test]
async fn a_caller_the_node_lets_reads_one_file_back_whole_at_the_length_the_agent_declared() {
  let moved = stand().await;
  let (status, headers, bytes) = moved.take(TESTER.1, NODE, "4", PATH).await;

  assert_eq!(status, StatusCode::OK, "{bytes:#?}");
  assert_eq!(headers["content-type"].to_str().unwrap(), "application/octet-stream");
  assert_eq!(headers["content-length"].to_str().unwrap(), FILE.len().to_string());
  assert_eq!(bytes, FILE);

  let asked = moved.asked.takes.lock().unwrap().last().cloned().unwrap();

  assert_eq!(asked.agent.as_deref(), Some(NODE));
  assert_eq!(asked.path, PATH);
  assert_eq!(asked.bearer, Some(format!("Bearer {HUB_TOKEN}")));
  assert_eq!(
    moved.asked.reads.lock().unwrap().clone(),
    [(NODE.to_owned(), Some(format!("Bearer {OWNERS_TOKEN}")))]
  );
}

#[tokio::test]
async fn a_transfer_takes_a_caller_s_own_token_and_a_caller_s_token_takes_no_other_route() {
  let moved = stand().await;

  let (status, body) = moved.put(TOKEN, NODE, "4", PATH, FILE).await;

  assert_eq!((status, body["code"].clone()), (StatusCode::FORBIDDEN, json!("forbidden")), "{body}");
  assert!(body["error"].as_str().unwrap().contains("a caller's own token"), "{body}");

  let (status, _, bytes) = moved.take(TOKEN, NODE, "4", PATH).await;
  let body = moved.refusal(&bytes);

  assert_eq!((status, body["code"].clone()), (StatusCode::FORBIDDEN, json!("forbidden")), "{body}");
  assert!(body["error"].as_str().unwrap().contains("a caller's own token"), "{body}");

  let (status, body) = moved.put("no-such-token", NODE, "4", PATH, FILE).await;

  assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

  let (status, body) = moved.stood.ask_with(Some(TESTER.1), Method::GET, "/nodes", None).await;

  assert_eq!((status, body["code"].clone()), (StatusCode::FORBIDDEN, json!("forbidden")), "{body}");
  assert!(body["error"].as_str().unwrap().contains("no other route"), "{body}");
  assert!(moved.asked.puts.lock().unwrap().is_empty());
  assert!(moved.asked.takes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_caller_the_node_does_not_let_on_and_a_person_whose_node_it_is_not_are_refused() {
  let moved = stand().await;
  let (status, body) = moved.put(OTHER.1, NODE, "4", PATH, FILE).await;

  assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
  assert_eq!(body["code"], "forbidden");
  assert!(body["error"].as_str().unwrap().contains(OTHER.0), "{body}");
  assert!(body["error"].as_str().unwrap().contains("move a file"), "{body}");

  let (status, _, bytes) = moved.take(TESTER.1, NODE, "5", PATH).await;
  let body = moved.refusal(&bytes);

  assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
  assert!(body["error"].as_str().unwrap().contains("account 4's, not account 5's"), "{body}");
  assert!(moved.asked.puts.lock().unwrap().is_empty());
  assert!(moved.asked.takes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_ssh_node_is_refused_as_one_a_transfer_does_not_reach() {
  let moved = stand().await;

  moved
    .stood
    .store()
    .add_node("desk-a", None, None, SshLogin::new("192.0.2.9", "someone"))
    .unwrap();
  moved.stood.store().set_callers("desk-a", Some(TESTER.0.into())).unwrap();

  let (status, body) = moved.put(TESTER.1, "desk-a", "4", PATH, FILE).await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
  assert_eq!(body["code"], "not-a-hub-node");
  assert!(moved.asked.puts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_the_service_does_not_hold_is_not_found_before_the_hub_is_asked() {
  let moved = stand().await;
  let (status, body) = moved.put(TESTER.1, "no-such-node", "4", PATH, FILE).await;

  assert_eq!((status, body["code"].clone()), (StatusCode::NOT_FOUND, json!("not-found")), "{body}");
  assert!(moved.asked.reads.lock().unwrap().is_empty());
  assert!(moved.asked.puts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_path_the_node_will_not_take_is_the_caller_s_bad_request() {
  let moved = stand().await;

  moved.refused_with(PutAnswer::Agent {
    code: "not-found".into(),
    message: MISSING.into(),
  });

  let (status, body) = moved.put(TESTER.1, NODE, "4", PATH, FILE).await;

  assert_eq!(
    (status, body["code"].clone()),
    (StatusCode::BAD_REQUEST, json!("bad-request")),
    "{body}"
  );
  assert!(body["error"].as_str().unwrap().contains(PATH), "{body}");
  assert!(body["error"].as_str().unwrap().contains(MISSING), "{body}");
}

#[tokio::test]
async fn bytes_the_agent_could_not_verify_are_a_failed_upstream_naming_the_mismatch() {
  let moved = stand().await;

  moved.refused_with(PutAnswer::Agent {
    code: "integrity".into(),
    message: UNVERIFIED.into(),
  });

  let (status, body) = moved.put(TESTER.1, NODE, "4", PATH, FILE).await;

  assert_eq!(
    (status, body["code"].clone()),
    (StatusCode::BAD_GATEWAY, json!("upstream-failed")),
    "{body}"
  );
  assert!(body["error"].as_str().unwrap().contains(UNVERIFIED), "{body}");
}

#[tokio::test]
async fn a_hub_that_refuses_the_token_is_a_failed_upstream_naming_the_hub() {
  let moved = stand().await;

  moved.refused_with(PutAnswer::Hub {
    status: 401,
    error: "unauthorized".into(),
  });

  let (status, body) = moved.put(TESTER.1, NODE, "4", PATH, FILE).await;

  assert_eq!(
    (status, body["code"].clone()),
    (StatusCode::BAD_GATEWAY, json!("upstream-failed")),
    "{body}"
  );
  assert!(body["error"].as_str().unwrap().contains("the hub refused the token"), "{body}");
}

#[tokio::test]
async fn a_file_the_node_does_not_hold_is_not_found_naming_its_path() {
  let moved = stand().await;

  moved.answered_with(TakeAnswer::Agent {
    code: "not-found".into(),
    message: NO_FILE.into(),
  });

  let (status, _, bytes) = moved.take(TESTER.1, NODE, "4", PATH).await;
  let body = moved.refusal(&bytes);

  assert_eq!((status, body["code"].clone()), (StatusCode::NOT_FOUND, json!("not-found")), "{body}");
  assert!(body["error"].as_str().unwrap().contains(PATH), "{body}");
}

#[tokio::test]
async fn the_rust_client_moves_one_file_each_way_as_the_caller_it_is_built_with() {
  let moved = stand().await;
  let client = NodeClient::new(&moved.stood.base, TESTER.1).unwrap();
  let written = client
    .upload_file(
      NODE,
      &UploadQuery {
        account: "4".into(),
        path: PATH.into(),
        total: FILE.len() as u64,
        checksum: CHECKSUM.into(),
      },
      FILE.to_vec(),
    )
    .await
    .unwrap();

  assert_eq!(written.node, NODE);
  assert_eq!(written.path, PATH);
  assert_eq!(written.total, FILE.len() as u64);
  assert_eq!(written.checksum, CHECKSUM);

  let (total, bytes) = client
    .download_file(
      NODE,
      &DownloadQuery {
        account: "4".into(),
        path: PATH.into(),
      },
    )
    .await
    .unwrap();
  let held = bytes.try_collect::<Vec<Vec<u8>>>().await.unwrap().concat();

  assert_eq!((total, held.as_slice()), (FILE.len() as u64, FILE));
}
