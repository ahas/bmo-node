//! A node service of the test's own: a scratch data root, an OpenSSH that ends as it is told, where asked
//! once the test opens its gate, a proc root the test writes, a road that prints the readings it is handed,
//! and the routes served on a free port and drained as the service drains once the test stops it.
//! Every answer a test reads is held to the committed OpenAPI document: its status is one the route
//! states, and its body the schema stated for that status.

use std::ffi::CString;
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use hardware::live::Road;
use hardware::roads::StreamEnd;
use node_server::Service;
use node_server::stop::{self, DRAIN, Drained};
use node_server::this_machine::ThisMachine;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use store::Store;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub const TOKEN: &str = "test-token";

/// The variable the stood service reads a process's scope from.
pub const SCOPE: &str = "TEST_SCOPE";

/// Two whole live readings and nothing more, then the run stays open until it is stopped.
pub fn reading_road() -> Road {
  Arc::new(|_source, _script, lines, stop| {
    Box::pin(async move {
      for (uptime, busy, seldom) in [(10, 0, true), (11, 50, false)] {
        for line in reading(uptime, busy, seldom).lines() {
          let _ = lines.send(line.to_owned()).await;
        }
      }

      stop.cancelled().await;

      StreamEnd {
        ok: false,
        failure: "stopped".into(),
      }
    })
  })
}

/// A road that fails at once, saying what it met.
pub fn failing_road(failure: &'static str) -> Road {
  Arc::new(move |_source, _script, _lines, _stop| {
    Box::pin(async move {
      StreamEnd {
        ok: false,
        failure: failure.into(),
      }
    })
  })
}

/// One reading of the live run, the seldom parts on it where `seldom`.
pub fn reading(uptime: i64, busy: i64, seldom: bool) -> String {
  let mut lines: Vec<String> = Vec::new();

  if seldom {
    lines.extend(
      [
        "@@host",
        "box",
        "@@kernel",
        "6.6",
        "@@layout",
        "/sys/devices/system/cpu/cpu0/topology/core_id:0",
      ]
      .map(str::to_owned),
    );
  }

  lines.extend([
    "@@uptime".to_owned(),
    uptime.to_string(),
    "@@stat".to_owned(),
    format!("cpu  {busy} 0 0 {} 0 0 0 0", uptime * 100 - busy),
    format!("cpu0 {busy} 0 0 {} 0 0 0 0", uptime * 100 - busy),
    "@@end".to_owned(),
  ]);
  lines.join("\n")
}

pub struct Stood {
  pub dir: tempfile::TempDir,
  pub service: Arc<Service>,
  pub base: String,
  pub client: reqwest::Client,
  /// The serving, which ends with how its drain ended once the service is stopped.
  pub served: JoinHandle<std::io::Result<Drained>>,
  /// Sent, or dropped with the stood service, to stop it.
  stop: Option<oneshot::Sender<()>>,
}

pub struct Stand {
  pub road: Road,
  pub ssh_code: i32,
  /// Whether each login waits at the gate until the test opens it (`Stood::login_at_gate`).
  pub ssh_gated: bool,
  /// How long the calls under way are given once the service is stopped.
  pub drain: Duration,
}

impl Default for Stand {
  fn default() -> Self {
    Self {
      road: reading_road(),
      ssh_code: 0,
      ssh_gated: false,
      drain: DRAIN,
    }
  }
}

impl Stand {
  /// The service stood and served.
  pub async fn up(self) -> Stood {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Arc::new(Store::open_in(&data).unwrap());
    let mut service =
      Service::new(store, TOKEN, data, Some("Lab machine".into())).with_scope_variable(Some(SCOPE.into()));
    let ssh = dir.path().join("ssh");

    // A gated login reads the gate, a named pipe, which holds it until the test opens the pipe's other end.
    let waits = if self.ssh_gated {
      make_pipe(&dir.path().join("gate"));
      "cat \"$(dirname \"$0\")/gate\" >/dev/null\n"
    } else {
      ""
    };

    std::fs::write(&ssh, format!("#!/bin/sh\n{waits}exit {}\n", self.ssh_code)).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    service.machine = node_check::Machine::new(&ssh, dir.path());
    service.road = self.road;
    service.here = ThisMachine::new(Some("host-a".into()), vec!["192.0.2.84".parse().unwrap()]);
    service.proc_root = dir.path().join("proc");
    fake_proc(&service.proc_root, 74.0, 24, 2_097_152, 25.0);

    let service = Arc::new(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = oneshot::channel::<()>();
    let served = tokio::spawn(stop::serve(
      listener,
      service.clone(),
      async move {
        let _ = stopped.await;
      },
      self.drain,
    ));

    Stood {
      dir,
      service,
      base,
      client: reqwest::Client::new(),
      served,
      stop: Some(stop),
    }
  }
}

fn make_pipe(path: &Path) {
  let named = CString::new(path.as_os_str().as_bytes()).unwrap();

  // SAFETY: `named` is a NUL-terminated path that outlives the call.
  assert_eq!(unsafe { libc::mkfifo(named.as_ptr(), 0o600) }, 0, "{}", path.display());
}

/// A proc root of the test's own: load, threads, memory and memory pressure as the kernel words them.
pub fn fake_proc(dir: &Path, load1: f64, threads: usize, available_kb: u64, memory_full60: f64) {
  std::fs::create_dir_all(dir.join("pressure")).unwrap();
  std::fs::write(dir.join("loadavg"), format!("{load1} 30.10 28.00 12/7136 80143\n")).unwrap();
  std::fs::write(
    dir.join("stat"),
    format!(
      "cpu  1 2 3 4\n{}intr 1\n",
      (0..threads).map(|cpu| format!("cpu{cpu} 1 2 3 4\n")).collect::<String>()
    ),
  )
  .unwrap();
  std::fs::write(
    dir.join("meminfo"),
    format!("MemTotal:       33554432 kB\nMemAvailable:   {available_kb} kB\nSwapTotal:      16777216 kB\nSwapFree:       0 kB\n"),
  )
  .unwrap();
  std::fs::write(
    dir.join("pressure/memory"),
    format!("some avg10=1.00 avg60=1.00 avg300=1.00 total=1\nfull avg10=1.00 avg60={memory_full60} avg300=1.00 total=1\n"),
  )
  .unwrap();
}

/// The committed OpenAPI document.
pub static CONTRACT: LazyLock<Value> = LazyLock::new(|| {
  let text =
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/openapi.json")).unwrap();

  serde_json::from_str(&text).unwrap()
});

/// The document's path a request's path stands under: each `{name}` segment standing for any one.
fn template(path: &str) -> String {
  let asked: Vec<&str> = path.split('?').next().unwrap_or_default().split('/').collect();
  let paths = CONTRACT["paths"].as_object().unwrap();

  paths
    .keys()
    .find(|template| {
      let parts: Vec<&str> = template.split('/').collect();

      parts.len() == asked.len()
        && parts.iter().zip(&asked).all(|(part, one)| part.starts_with('{') || part == one)
    })
    .cloned()
    .unwrap_or_else(|| panic!("the document states no path for {path}"))
}

/// A response object, its reference followed where it names a shared one.
fn followed(response: &Value) -> &Value {
  match response["$ref"].as_str() {
    Some(reference) => {
      let name = reference.rsplit('/').next().unwrap();

      &CONTRACT["components"]["responses"][name]
    }
    None => response,
  }
}

/// An answer held to the document: a status the route states, and a body of the schema stated for it.
pub fn holds_to_contract(method: &Method, path: &str, status: StatusCode, body: &Value) {
  if status == StatusCode::NOT_FOUND && body["code"] == "no-route" {
    return;
  }

  let template = template(path);
  let operation = &CONTRACT["paths"][&template][method.as_str().to_lowercase()];

  assert!(operation.is_object(), "the document states no {method} {template}");

  let response = followed(&operation["responses"][status.as_str()]);

  assert!(response.is_object(), "{method} {template} states no {status} answer: {body}");

  let Some(schema) = response["content"]["application/json"].get("schema") else {
    assert_eq!(body, &json!(""), "{method} {template} answers {status} with no body");
    return;
  };
  let rooted = json!({ "components": CONTRACT["components"], "allOf": [schema] });
  let validator = jsonschema::validator_for(&rooted).unwrap();
  let faults: Vec<String> = validator.iter_errors(body).map(|fault| fault.to_string()).collect();

  assert!(
    faults.is_empty(),
    "{method} {path} {status} is not its stated schema: {faults:#?}\n{body}"
  );
}

impl Stood {
  /// A call with the service's token, its status and its JSON, held to the document.
  pub async fn ask(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    self.ask_with(Some(TOKEN), method, path, body).await
  }

  pub async fn ask_with(
    &self,
    token: Option<&str>,
    method: Method,
    path: &str,
    body: Option<Value>,
  ) -> (StatusCode, Value) {
    let mut request = self.client.request(method.clone(), format!("{}{path}", self.base));

    if let Some(token) = token {
      request = request.bearer_auth(token);
    }

    if let Some(body) = body {
      request = request.json(&body);
    }

    let answer = request.send().await.unwrap();
    let status = answer.status();
    let text = answer.text().await.unwrap();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));

    holds_to_contract(&method, path, status, &body);
    (status, body)
  }

  /// A call that must answer `status`.
  pub async fn answered(
    &self,
    method: Method,
    path: &str,
    body: Option<Value>,
    status: StatusCode,
  ) -> Value {
    let (got, answer) = self.ask(method.clone(), path, body).await;

    assert_eq!(got, status, "{method} {path}: {answer}");
    answer
  }

  pub async fn get(&self, path: &str) -> Value {
    self.answered(Method::GET, path, None, StatusCode::OK).await
  }

  pub async fn create(&self, path: &str, body: Value) -> Value {
    self.answered(Method::POST, path, Some(body), StatusCode::CREATED).await
  }

  pub async fn patch(&self, path: &str, body: Value) -> Value {
    self.answered(Method::PATCH, path, Some(body), StatusCode::OK).await
  }

  pub async fn delete(&self, path: &str) {
    self.answered(Method::DELETE, path, None, StatusCode::NO_CONTENT).await;
  }

  /// The code a call is refused with, held to the status the code is answered with.
  pub async fn refused(&self, method: Method, path: &str, body: Option<Value>) -> String {
    let (status, answer) = self.ask(method, path, body).await;
    let code = answer["code"].as_str().unwrap_or_else(|| panic!("{path} was not refused: {answer}"));
    let expected = serde_json::from_value::<bmo_node_client::ErrorCode>(json!(code)).unwrap().status();

    assert_eq!(status.as_u16(), expected, "{code}");
    assert!(answer["error"].as_str().is_some_and(|error| !error.is_empty()));
    code.to_owned()
  }

  pub fn store(&self) -> &Store {
    &self.service.store
  }

  pub fn home(&self) -> PathBuf {
    self.dir.path().to_path_buf()
  }

  /// Told to stop, as the signal tells the service.
  pub fn stop(&mut self) {
    if let Some(stop) = self.stop.take() {
      let _ = stop.send(());
    }
  }

  /// The gate's open end, once a gated login waits at it: the login ends as soon as it is dropped.
  pub async fn login_at_gate(&self) -> File {
    let gate = self.dir.path().join("gate");

    // Opening a named pipe to write waits until a reader has it open.
    tokio::task::spawn_blocking(move || File::options().write(true).open(gate).unwrap())
      .await
      .unwrap()
  }
}
