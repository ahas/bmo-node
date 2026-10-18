//! A node service of the test's own: a scratch data root, an OpenSSH that ends as it is told, a proc root
//! the test writes, a road that prints the readings it is handed, and the routes served on a free port.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hardware::live::Road;
use hardware::roads::StreamEnd;
use node_server::this_machine::ThisMachine;
use node_server::{Service, app};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use store::Store;

pub const TOKEN: &str = "test-token";

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
}

pub struct Stand {
  pub road: Road,
  pub ssh_code: i32,
}

impl Default for Stand {
  fn default() -> Self {
    Self {
      road: reading_road(),
      ssh_code: 0,
    }
  }
}

impl Stand {
  /// The service stood and served.
  pub async fn up(self) -> Stood {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Arc::new(Store::open_in(&data).unwrap());
    let mut service = Service::new(store, TOKEN, data, Some("Lab machine".into()));
    let ssh = dir.path().join("ssh");

    std::fs::write(&ssh, format!("#!/bin/sh\nexit {}\n", self.ssh_code)).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    service.machine = node_check::Machine::new(&ssh, dir.path());
    service.road = self.road;
    service.here = ThisMachine::new(Some("host-a".into()), vec!["192.0.2.84".parse().unwrap()]);
    service.proc_root = dir.path().join("proc");
    fake_proc(&service.proc_root, 74.0, 24, 2_097_152, 25.0);

    let service = Arc::new(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = app(service.clone());

    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });

    Stood {
      dir,
      service,
      base,
      client: reqwest::Client::new(),
    }
  }
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

impl Stood {
  /// A call with the service's token, its status and its JSON.
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
    let mut request = self.client.request(method, format!("{}{path}", self.base));

    if let Some(token) = token {
      request = request.bearer_auth(token);
    }

    if let Some(body) = body {
      request = request.json(&body);
    }

    let answer = request.send().await.unwrap();
    let status = answer.status();
    let text = answer.text().await.unwrap();

    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
  }

  pub async fn get(&self, path: &str) -> Value {
    let (status, answer) = self.ask(Method::GET, path, None).await;

    assert_eq!(status, StatusCode::OK, "{path}: {answer}");
    answer
  }

  pub async fn post(&self, path: &str, body: Value) -> Value {
    let (status, answer) = self.ask(Method::POST, path, Some(body)).await;

    assert_eq!(status, StatusCode::OK, "{path}: {answer}");
    answer
  }

  pub fn store(&self) -> &Store {
    &self.service.store
  }

  pub fn home(&self) -> PathBuf {
    self.dir.path().to_path_buf()
  }
}

/// The refusal an answer is, by its code.
pub fn refused(answer: &Value) -> &str {
  assert_eq!(answer["answered"], false, "{answer}");
  answer["refusal"]["code"].as_str().unwrap()
}
