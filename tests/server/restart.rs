//! The `bmo-node` service as a process, sent SIGTERM in the middle of a node check and started again on the
//! same store (docs/api.md, 「Stopping」): the signal goes to the service alone, as its unit sends it. A
//! check that finishes within the drain answers and is kept for the next run; one that outlives the drain is
//! cut with the process, its caller sees the connection drop, nothing of it is written, and the next run
//! checks the node again.

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use policy::node_states::NodeState;
use serde_json::{Value, json};
use store::Store;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::server::stop::refuses;
use crate::server::support::make_pipe;
use crate::support::binary::built;

const TOKEN: &str = "test-token";

/// How long a test waits on what should come at once before it fails, so a fault fails rather than hangs.
const BOUND: Duration = Duration::from_secs(20);

/// A scratch place for the service: its configuration, its data root, and an OpenSSH on its path that waits
/// at a named pipe until the test opens the pipe's other end, then logs in.
struct Place {
  dir: tempfile::TempDir,
  config: PathBuf,
  base: String,
}

impl Place {
  fn new(drain_ms: u64) -> Self {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let ssh = bin.join("ssh");
    // A port free now, taken again by each run of the service.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = dir.path().join("node.toml");

    std::fs::create_dir(&bin).unwrap();
    make_pipe(&dir.path().join("gate"));
    std::fs::write(&ssh, "#!/bin/sh\ncat \"$(dirname \"$0\")/../gate\" >/dev/null\nexit 0\n").unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
      &config,
      toml_text(&[
        ("listen", json!(format!("127.0.0.1:{port}"))),
        ("token", json!(TOKEN)),
        ("data_root", json!(dir.path().join("data").display().to_string())),
        ("machine_label", json!("Lab machine")),
        ("drain_ms", json!(drain_ms)),
      ]),
    )
    .unwrap();

    Self {
      dir,
      config,
      base: format!("http://127.0.0.1:{port}"),
    }
  }

  /// The service started over this place and asked until it answers ready.
  async fn start(&self) -> Running {
    let path = format!(
      "{}:{}",
      self.dir.path().join("bin").display(),
      std::env::var("PATH").unwrap_or_default()
    );
    let child = Command::new(built("node-server", "bmo-node"))
      .arg("--config")
      .arg(&self.config)
      .arg("serve")
      .env("PATH", path)
      .env("HOME", self.dir.path())
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .process_group(0)
      .kill_on_drop(true)
      .spawn()
      .unwrap();
    let running = Running { child };

    for _ in 0..2_000 {
      if let Ok(answer) = self.client().get(format!("{}/health", self.base)).bearer_auth(TOKEN).send().await
        && answer.json::<Value>().await.is_ok_and(|health| health["ready"] == json!(true))
      {
        return running;
      }

      tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("the service never answered ready");
  }

  fn client(&self) -> reqwest::Client {
    reqwest::Client::new()
  }

  async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> reqwest::Result<Value> {
    let mut request = self.client().request(method, format!("{}{path}", self.base)).bearer_auth(TOKEN);

    if let Some(body) = body {
      request = request.json(&body);
    }

    request.send().await?.error_for_status()?.json().await
  }

  /// A node check of `lab` asked on a task of its own, and the gate's open end once its login waits there.
  async fn check_held(&self) -> (tokio::task::JoinHandle<reqwest::Result<Value>>, File) {
    let place = (self.base.clone(), self.client());
    let check = tokio::spawn(async move {
      let (base, client) = place;

      client
        .post(format!("{base}/node-checks"))
        .bearer_auth(TOKEN)
        .json(&json!({ "nodeNames": ["lab"] }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await
    });

    (check, tokio::time::timeout(BOUND, self.login_at_gate()).await.unwrap())
  }

  /// The gate's open end, once a login waits at it: the login ends as soon as it is dropped.
  async fn login_at_gate(&self) -> File {
    let gate = self.dir.path().join("gate");

    tokio::task::spawn_blocking(move || File::options().write(true).open(gate).unwrap())
      .await
      .unwrap()
  }

  async fn lab_state(&self) -> Value {
    self.call(reqwest::Method::GET, "/nodes/lab", None).await.unwrap()["state"].clone()
  }
}

fn toml_text(pairs: &[(&str, Value)]) -> String {
  pairs.iter().map(|(key, value)| format!("{key} = {value}\n")).collect()
}

/// One run of the service, taken down with everything it started when the test lets it go.
struct Running {
  child: Child,
}

impl Running {
  fn pid(&self) -> i32 {
    self.child.id().unwrap() as i32
  }

  /// SIGTERM to the service's own process alone, as its unit sends it.
  fn terminate(&self) {
    // SAFETY: a signal sent to a process this test started and still holds.
    assert_eq!(unsafe { libc::kill(self.pid(), libc::SIGTERM) }, 0);
  }

  fn still_running(&mut self) -> bool {
    self.child.try_wait().unwrap().is_none()
  }

  /// How the process ended, and what it printed.
  async fn ended(&mut self) -> (bool, String) {
    let status = tokio::time::timeout(BOUND, self.child.wait()).await.unwrap().unwrap();
    let mut printed = String::new();

    self.child.stdout.take().unwrap().read_to_string(&mut printed).await.unwrap();
    (status.success(), printed)
  }
}

impl Drop for Running {
  fn drop(&mut self) {
    // The service's process group: what it started and left behind, a login waiting at the gate among them.
    if let Some(pid) = self.child.id() {
      // SAFETY: the group this test started the service as the leader of.
      unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }

    let _ = self.child.start_kill();
  }
}

async fn with_lab(place: &Place) {
  place
    .call(
      reqwest::Method::POST,
      "/nodes",
      Some(json!({ "name": "lab", "host": "192.0.2.4", "user": "someone" })),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_check_under_way_at_sigterm_finishes_and_the_next_run_starts_from_what_it_wrote() {
  let place = Place::new(600_000);
  let mut first = place.start().await;

  with_lab(&place).await;

  let (check, gate) = place.check_held().await;

  first.terminate();

  assert!(refuses(&place.base).await, "the service still takes callers after SIGTERM");
  assert!(first.still_running(), "the service ended with a call under way");

  drop(gate);

  let checked = tokio::time::timeout(BOUND, check).await.unwrap().unwrap().unwrap();

  assert_eq!(
    (checked["items"][0]["name"].clone(), checked["items"][0]["state"].clone()),
    (json!("lab"), json!("connected"))
  );

  let (succeeded, printed) = first.ended().await;

  assert!(succeeded && printed.contains("stopped, every call finished"), "{printed}");

  let _second = place.start().await;

  assert_eq!(place.lab_state().await, json!("connected"));
}

#[tokio::test]
async fn a_check_outliving_the_drain_is_cut_with_the_process_and_the_next_run_checks_again() {
  let place = Place::new(100);
  let mut first = place.start().await;

  with_lab(&place).await;

  let (check, gate) = place.check_held().await;

  first.terminate();

  let (succeeded, printed) = first.ended().await;

  assert!(succeeded && printed.contains("cut after 100 ms"), "{printed}");
  assert!(
    tokio::time::timeout(BOUND, check).await.unwrap().unwrap().is_err(),
    "the cut call was answered"
  );
  drop(first);
  drop(gate);

  let kept = Store::open_in(&place.dir.path().join("data")).unwrap();

  assert_eq!(
    kept.node("lab").unwrap().unwrap().state,
    NodeState::Unknown,
    "the cut check wrote its state"
  );
  drop(kept);

  let _second = place.start().await;

  // The next run's first pass checks the node, its login at the gate.
  drop(tokio::time::timeout(BOUND, place.login_at_gate()).await.unwrap());

  for _ in 0..2_000 {
    if place.lab_state().await == json!("connected") {
      return;
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  panic!("the next run never checked the node again");
}
