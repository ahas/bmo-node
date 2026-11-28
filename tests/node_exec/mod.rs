//! The hosted tool `node-exec` (docs/tools.md): who may run a command on which node, and a command run on a
//! hub node through its hub, each against a stand-in of agency, the node service and the hub on one port.

mod approval;
mod call;
mod caller;
mod restart;
mod script;
mod secrets;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_node_client::NodeClient;
use bmo_node_client::nodes::{LoginShell, NodeKind, Target};
use node_check::Machine;
use node_exec_tool::caller::{Agency, Session};
use node_exec_tool::{ENTRY, Ground, Handed};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The hub's control token in the stand-in's token file; no answer may carry it.
pub const HUB_TOKEN: &str = "hub-secret-token";

/// One exec the hub was asked: its agent, its bearer and its body.
pub type Exec = (Option<String>, Option<String>, Value);

/// One question the job service was asked, and the replies it holds.
pub type Asked = (Value, Vec<Value>);

/// What the stand-in holds and what it was asked.
#[derive(Clone, Default)]
pub struct World {
  /// Supervisors by name, each with its `ownerAccount`.
  pub supervisors: Arc<Mutex<HashMap<String, Value>>>,
  /// Role agents as agency lists them.
  pub role_agents: Arc<Mutex<Vec<Value>>>,
  /// Agency's records of a person's node.
  pub person_nodes: Arc<Mutex<Vec<Value>>>,
  /// The nodes' targets by name.
  pub targets: Arc<Mutex<HashMap<String, Target>>>,
  /// The frames the hub's exec answers with.
  pub frames: Arc<Mutex<Vec<String>>>,
  /// Every exec the hub was asked.
  pub execs: Arc<Mutex<Vec<Exec>>>,
  /// The job service's questions as asked, by number from 1, and the replies each holds.
  pub questions: Arc<Mutex<Vec<Asked>>>,
}

async fn ask(State(world): State<World>, axum::Json(body): axum::Json<Value>) -> Response {
  let mut questions = world.questions.lock().unwrap();
  let id = questions.len() as i64 + 1;
  let mut question = body.clone();

  question["id"] = json!(id);
  question["state"] = json!("open");
  questions.push((question.clone(), vec![]));

  (StatusCode::CREATED, axum::Json(question)).into_response()
}

async fn question(State(world): State<World>, Path(id): Path<usize>) -> Response {
  match world.questions.lock().unwrap().get(id - 1) {
    Some((question, _)) => axum::Json(question.clone()).into_response(),
    None => StatusCode::NOT_FOUND.into_response(),
  }
}

async fn replies(State(world): State<World>, Path(id): Path<usize>) -> Response {
  match world.questions.lock().unwrap().get(id - 1) {
    Some((_, replies)) => axum::Json(json!({ "items": replies, "more": false })).into_response(),
    None => StatusCode::NOT_FOUND.into_response(),
  }
}

pub fn frame(event: &str, data: Value) -> String {
  format!("event: {event}\ndata: {data}\n\n")
}

pub fn out(stream: &str, bytes: &[u8], seq: i64) -> String {
  frame(stream, json!({ "seq": seq, "chunk": { "base64": STANDARD.encode(bytes) } }))
}

async fn supervisor(State(world): State<World>, Path(name): Path<String>) -> Response {
  match world.supervisors.lock().unwrap().get(&name) {
    Some(owner) => axum::Json(json!({ "name": name, "ownerAccount": owner })).into_response(),
    None => (
      StatusCode::NOT_FOUND,
      axum::Json(json!({ "code": "not-found", "error": "no supervisor" })),
    )
      .into_response(),
  }
}

async fn role_agents(State(world): State<World>) -> Response {
  axum::Json(json!({ "items": world.role_agents.lock().unwrap().clone() })).into_response()
}

async fn person_nodes(State(world): State<World>) -> Response {
  axum::Json(json!({ "items": world.person_nodes.lock().unwrap().clone(), "more": false })).into_response()
}

async fn target(State(world): State<World>, Path(name): Path<String>) -> Response {
  match world.targets.lock().unwrap().get(&name) {
    Some(target) => axum::Json(target.clone()).into_response(),
    None => (
      StatusCode::NOT_FOUND,
      axum::Json(json!({ "code": "not-found", "error": format!("no node {name}") })),
    )
      .into_response(),
  }
}

async fn exec(
  State(world): State<World>,
  Query(query): Query<HashMap<String, String>>,
  headers: HeaderMap,
  body: axum::Json<Value>,
) -> Response {
  let bearer = headers.get("authorization").and_then(|value| value.to_str().ok()).map(str::to_owned);

  world.execs.lock().unwrap().push((query.get("agent").cloned(), bearer, body.0));

  let stream = world.frames.lock().unwrap().concat();

  ([("content-type", "text/event-stream")], Body::from(stream)).into_response()
}

/// A hub standing for one command: it answers the exec with the stream's head and `frames`, says so on
/// `streaming`, and stops once `stop` is sent or dropped, closing its port and the command's connection in the
/// middle of the stream, as a hub process stopped there does.
pub struct HeldHub {
  pub url: String,
  pub addr: SocketAddr,
  pub streaming: oneshot::Receiver<()>,
  pub stop: oneshot::Sender<()>,
}

pub async fn held_hub(frames: String) -> HeldHub {
  let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.unwrap();
  let addr = listener.local_addr().unwrap();
  let (said, streaming) = oneshot::channel();
  let (stop, stopped) = oneshot::channel::<()>();

  tokio::spawn(async move {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut asked = Vec::new();
    let mut buffer = [0u8; 4096];

    // The whole request is read first, so closing leaves nothing unread that would reset the connection
    // before the frames are read.
    loop {
      let read = socket.read(&mut buffer).await.unwrap();

      if read == 0 {
        break;
      }

      asked.extend_from_slice(&buffer[..read]);

      let text = String::from_utf8_lossy(&asked).into_owned();
      let Some(head) = text.find("\r\n\r\n") else {
        continue;
      };
      let length = text[..head]
        .lines()
        .find_map(|line| {
          line.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().to_owned())
        })
        .and_then(|length| length.parse::<usize>().ok())
        .unwrap_or(0);

      if asked.len() >= head + 4 + length {
        break;
      }
    }

    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
    let chunk = format!("{:x}\r\n{frames}\r\n", frames.len());

    socket.write_all(format!("{head}{chunk}").as_bytes()).await.unwrap();
    socket.flush().await.unwrap();
    let _ = said.send(());
    let _ = stopped.await;
    drop(socket);
    drop(listener);
  });

  HeldHub {
    url: format!("http://{addr}"),
    addr,
    streaming,
    stop,
  }
}

/// A hub node's target, reached through the stand-in's hub at `url` as the agent `agent`.
pub fn hub_target(node: &str, url: &str, agent: &str, shell: LoginShell) -> Target {
  Target {
    node_name: node.into(),
    kind: NodeKind::Hub,
    role: None,
    host: None,
    port: None,
    user: None,
    key_path: None,
    shell: Some(shell),
    ssh_options: Vec::new(),
    ssh_args: Vec::new(),
    hub_id: Some(1),
    hub_address: Some(url.into()),
    hub_token_file: Some("~/hub.env".into()),
    agent_name: Some(agent.into()),
  }
}

/// The stand-in standing, and the ground a call is answered over.
pub struct Stood {
  pub world: World,
  pub url: String,
  pub ground: Ground,
  /// The stand-in's data root's `env/`, the platform's secrets, with each account's under `users/`.
  pub env: std::path::PathBuf,
  _home: tempfile::TempDir,
}

/// A listener at `addr`. A port named again after its stand-in stopped may be held a moment by another socket of
/// the suite running beside it, so its bind is tried until the port comes free, for two seconds at most.
async fn bind(addr: SocketAddr) -> tokio::net::TcpListener {
  for _ in 0..100 {
    match tokio::net::TcpListener::bind(addr).await {
      Err(why) if why.kind() == std::io::ErrorKind::AddrInUse && addr.port() != 0 => {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
      }
      bound => return bound.unwrap(),
    }
  }

  panic!("{addr} stayed held by another socket");
}

/// The stand-in of agency, the node service and the hub over `world`, listening at `addr`, a port of 0 taking
/// a free one; the address it listens at, and its task, whose abort stops it and closes its port.
pub async fn serve_at(world: &World, addr: SocketAddr) -> (SocketAddr, JoinHandle<()>) {
  let app = Router::new()
    .route("/supervisors/{name}", get(supervisor))
    .route("/role-agents", get(role_agents))
    .route("/person-nodes", get(person_nodes))
    .route("/nodes/{name}/target", get(target))
    .route("/exec", post(exec))
    .route("/questions", post(ask))
    .route("/questions/{id}", get(question))
    .route("/questions/{id}/replies", get(replies))
    .with_state(world.clone());
  let listener = bind(addr).await;
  let addr = listener.local_addr().unwrap();

  (addr, tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }))
}

pub async fn stood() -> Stood {
  let world = World::default();
  let (addr, _) = serve_at(&world, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await;
  let url = format!("http://{addr}");
  let home = tempfile::tempdir().unwrap();

  std::fs::write(home.path().join("hub.env"), format!("JAKE_CONTROL_TOKEN={HUB_TOKEN}\n")).unwrap();

  let env = home.path().join("env");

  std::fs::create_dir(&env).unwrap();

  let ground = Ground {
    nodes: NodeClient::new(&url, "node-token"),
    agency: Ok(Agency::new(&url, "agency-token")),
    secrets: Ok(env.clone()),
    questions: Ok(node_exec_tool::approval::Questions::new(&url, "job-token")),
    machine: Machine::new("ssh", home.path()),
  };

  Stood {
    world,
    url,
    ground,
    env,
    _home: home,
  }
}

/// A session held by `holder`, with the grants named.
pub fn session(holder: &str, grants: &[&str]) -> Session {
  serde_json::from_value(json!({
    "holderName": holder,
    "grants": grants.iter().map(|resource| json!({ "resource": resource })).collect::<Vec<_>>(),
  }))
  .unwrap()
}

/// A call of the tool's entry by a session held by `holder`, with the grants named.
pub fn handed(holder: &str, grants: &[&str], arguments: Value) -> Handed {
  Handed {
    entry: ENTRY.into(),
    arguments,
    session: session(holder, grants),
  }
}

/// The stand-in with the node `pc-four`, account 4's, on a Windows host behind the agent `agent_w`, and the
/// supervisor `pc-agent` of account 4.
pub async fn owned() -> Stood {
  let stood = stood().await;
  let target = hub_target("pc-four", &stood.url, "agent_w", LoginShell::Powershell);

  stood.world.targets.lock().unwrap().insert("pc-four".into(), target);
  stood.world.supervisors.lock().unwrap().insert("pc-agent".into(), json!(4));
  stood
    .world
    .person_nodes
    .lock()
    .unwrap()
    .push(json!({ "ownerAccount": "4", "node": "pc-four", "setAt": 1 }));
  stood
}
