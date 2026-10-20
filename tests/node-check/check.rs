//! A node's check against a hub and an OpenSSH stood in for (docs/nodes.md, 「How a node's state is
//! known」), and a pass writing what each check met through the rows it is handed.

use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use node_check::hub::{self, HubAnswer};
use node_check::pass::{Checked, NodeRows, check_node, check_nodes};
use node_check::rows::{Hub, NodeRow};
use node_check::{Machine, Met, NodeCheck, NodeState};
use serde_json::{Value, json};
use tempfile::TempDir;

const TOKEN: &str = "secret-token-value";

/// A hub that answers its status the way the real one does, holding to the token it was given, and
/// counts how often it was asked.
struct StoodHub {
  url: String,
  status: Arc<Mutex<(StatusCode, Value)>>,
  asked: Arc<Mutex<usize>>,
}

impl StoodHub {
  async fn new(agents: &[&str]) -> Self {
    let status = Arc::new(Mutex::new((StatusCode::OK, attached(agents))));
    let asked = Arc::new(Mutex::new(0));
    let (answering, counting) = (status.clone(), asked.clone());
    let app = Router::new().route(
      "/status",
      get(move |headers: HeaderMap| async move {
        *counting.lock().unwrap() += 1;

        if headers.get("authorization").and_then(|value| value.to_str().ok())
          != Some(&format!("Bearer {TOKEN}"))
        {
          return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "error": "unauthorized" })))
            .into_response();
        }

        let (code, body) = answering.lock().unwrap().clone();

        (code, axum::Json(body)).into_response()
      }),
    );
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    Self { url, status, asked }
  }

  fn answer(&self, code: StatusCode, body: Value) {
    *self.status.lock().unwrap() = (code, body);
  }

  fn asked(&self) -> usize {
    *self.asked.lock().unwrap()
  }
}

// The status the hub answers with these agents attached.
fn attached(names: &[&str]) -> Value {
  json!({
    "run": "hub-1",
    "attached": !names.is_empty(),
    "agents": names.iter().map(|name| json!({ "name": name, "agentVersion": "0.1.0", "inFlight": [] })).collect::<Vec<_>>(),
  })
}

/// A home holding the hub's token file, and an OpenSSH that ends as it is told and keeps the
/// arguments it was started with.
struct Scratch {
  dir: TempDir,
}

impl Scratch {
  fn new() -> Self {
    let scratch = Self {
      dir: tempfile::tempdir().unwrap(),
    };

    scratch.token_file("JAKE_SECRET=other\nJAKE_CONTROL_TOKEN=\"secret-token-value\"\n");
    scratch.ssh(0, "");

    scratch
  }

  fn path(&self, name: &str) -> PathBuf {
    self.dir.path().join(name)
  }

  fn token_file(&self, text: &str) {
    std::fs::create_dir_all(self.path(".local/share/jake")).unwrap();
    std::fs::write(self.path(".local/share/jake/hub.env"), text).unwrap();
  }

  fn ssh(&self, code: i32, said: &str) {
    let script = format!(
      "#!/bin/sh\necho \"$@\" >> '{}'\nprintf '%s' '{said}' >&2\nexit {code}\n",
      self.path("ssh-args").display()
    );
    let path = self.path("ssh");

    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
  }

  fn logins(&self) -> Vec<String> {
    std::fs::read_to_string(self.path("ssh-args"))
      .unwrap_or_default()
      .lines()
      .map(str::to_owned)
      .collect()
  }

  fn machine(&self) -> Machine {
    Machine::new(self.path("ssh"), self.dir.path())
  }
}

fn ssh_row(name: &str, key: Option<&str>) -> NodeRow {
  NodeRow {
    name: name.into(),
    kind: "ssh".into(),
    host: Some("192.0.2.82".into()),
    port: Some(2222),
    user: Some("someone".into()),
    key_path: key.map(str::to_owned),
    agent: None,
    hub_url: None,
    token_file: None,
    state: NodeState::Unknown,
    detail: None,
  }
}

fn jake_row(name: &str, hub: &str, agent: &str) -> NodeRow {
  NodeRow {
    name: name.into(),
    kind: "hub".into(),
    host: None,
    port: None,
    user: None,
    key_path: None,
    agent: Some(agent.into()),
    hub_url: Some(hub.into()),
    token_file: Some("~/.local/share/jake/hub.env".into()),
    state: NodeState::Unknown,
    detail: None,
  }
}

async fn met(machine: &Machine, node: &NodeRow) -> Met {
  let check = check_node(machine, node).await;

  Met {
    state: check.state,
    detail: check.detail,
  }
}

#[tokio::test]
async fn a_key_named_but_missing_is_said_without_trying_the_login() {
  let scratch = Scratch::new();

  assert_eq!(
    met(&scratch.machine(), &ssh_row("pc-node", Some("~/.ssh/id_home"))).await,
    Met::new(NodeState::Error, "no key at ~/.ssh/id_home")
  );
  assert!(scratch.logins().is_empty());
}

#[tokio::test]
async fn an_ssh_node_is_one_batch_login_running_nothing_read_off_what_openssh_said() {
  let scratch = Scratch::new();

  std::fs::create_dir_all(scratch.path(".ssh")).unwrap();
  std::fs::write(scratch.path(".ssh/id_home"), "").unwrap();

  assert_eq!(
    met(&scratch.machine(), &ssh_row("pc-node", Some("~/.ssh/id_home"))).await,
    Met::connected()
  );

  let login = &scratch.logins()[0];

  assert!(login.starts_with("-o BatchMode=yes"));
  assert!(login.contains(&format!("-o IdentitiesOnly=yes -i {}", scratch.path(".ssh/id_home").display())));
  assert!(login.ends_with("-p 2222 someone@192.0.2.82 exit"));

  scratch.ssh(255, "someone@192.0.2.82: Permission denied (publickey).");
  assert_eq!(
    met(&scratch.machine(), &ssh_row("pc-node", None)).await,
    Met::new(NodeState::Auth, "someone@192.0.2.82: Permission denied (publickey).")
  );
}

#[tokio::test]
async fn its_agent_in_the_hub_s_list_is_connected() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["agent_a", "alpha"]).await;

  assert_eq!(
    met(&scratch.machine(), &jake_row("agent-a", &hub.url, "agent_a")).await,
    Met::connected()
  );
}

#[tokio::test]
async fn its_agent_missing_from_the_hub_s_list_is_disconnected() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["alpha"]).await;

  assert_eq!(
    met(&scratch.machine(), &jake_row("agent-a", &hub.url, "agent_a")).await,
    Met::new(NodeState::Disconnected, "the agent is not attached to its hub")
  );
}

#[tokio::test]
async fn a_status_with_no_agents_list_cannot_be_read() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&[]).await;

  hub.answer(StatusCode::OK, json!({ "attached": true, "agent": { "name": "agent_a" } }));

  assert_eq!(
    met(&scratch.machine(), &jake_row("agent-a", &hub.url, "agent_a")).await,
    Met::new(NodeState::Error, "the hub's status could not be read")
  );

  hub.answer(StatusCode::INTERNAL_SERVER_ERROR, json!({}));

  assert_eq!(
    met(&scratch.machine(), &jake_row("agent-a", &hub.url, "agent_a")).await,
    Met::new(NodeState::Error, "the hub answered 500")
  );
}

#[tokio::test]
async fn a_refused_token_a_silent_hub_and_an_unreadable_token_file_each_say_so_never_with_the_token() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["agent_a"]).await;
  let machine = scratch.machine();
  let node = jake_row("agent-a", &hub.url, "agent_a");

  scratch.token_file("JAKE_CONTROL_TOKEN=wrong\n");
  let refused = met(&machine, &node).await;

  let silent_url = {
    let held = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();

    format!("http://{}", held.local_addr().unwrap())
  };
  scratch.token_file(&format!("JAKE_CONTROL_TOKEN={TOKEN}\n"));
  let silent = met(&machine, &jake_row("agent-a", &silent_url, "agent_a")).await;

  std::fs::remove_file(scratch.path(".local/share/jake/hub.env")).unwrap();
  let unread = met(&machine, &node).await;

  scratch.token_file("JAKE_OTHER=1\n");
  let empty = met(&machine, &node).await;

  assert_eq!(refused, Met::new(NodeState::Auth, "the hub refused the token (401)"));
  assert_eq!(silent.state, NodeState::Unreachable);
  assert_eq!(
    unread,
    Met::new(NodeState::Error, "the token file ~/.local/share/jake/hub.env cannot be read")
  );
  assert_eq!(
    empty,
    Met::new(
      NodeState::Error,
      "the token file ~/.local/share/jake/hub.env holds no JAKE_CONTROL_TOKEN"
    )
  );

  for one in [refused, silent, unread, empty] {
    assert!(!format!("{one:?}").contains(TOKEN));
  }
}

#[tokio::test]
async fn a_hub_s_answer_names_its_agents_or_what_its_nodes_met() {
  let scratch = Scratch::new();
  let stood = StoodHub::new(&["agent_a", "alpha"]).await;
  let answer = hub::ask(
    &scratch.machine(),
    &Hub {
      url: stood.url.clone(),
      token_file: "~/.local/share/jake/hub.env".into(),
    },
  )
  .await;

  assert_eq!(answer.agents(), ["agent_a", "alpha"]);
  assert_eq!(
    serde_json::to_value(&answer).unwrap(),
    json!({ "agents": ["agent_a", "alpha"], "departed": [] })
  );
  // An answer handed back from another program, which names no departures for a hub without a record.
  assert_eq!(
    serde_json::from_value::<HubAnswer>(json!({ "agents": ["alpha"] })).unwrap().departed(),
    [] as [String; 0]
  );
  assert_eq!(
    serde_json::to_value(HubAnswer::Failed(Met::new(NodeState::Auth, "refused"))).unwrap(),
    json!({ "state": "auth", "detail": "refused" })
  );
}

/// The node list held in memory, written as a store writes it.
#[derive(Default)]
struct Rows {
  hubs: Vec<Hub>,
  nodes: Mutex<Vec<NodeRow>>,
  recorded: Mutex<Vec<(String, NodeCheck)>>,
}

impl NodeRows for Rows {
  type Error = std::convert::Infallible;

  async fn hubs(&self) -> Result<Vec<Hub>, Self::Error> {
    Ok(self.hubs.clone())
  }

  async fn list(&self) -> Result<Vec<NodeRow>, Self::Error> {
    Ok(self.nodes.lock().unwrap().clone())
  }

  async fn add_agent(&self, hub: &str, agent: &str) -> Result<(), Self::Error> {
    let mut nodes = self.nodes.lock().unwrap();

    if !nodes.iter().any(|node| node.jake() == Some((hub, agent))) {
      nodes.push(jake_row(&agent.replace('_', "-"), hub, agent));
    }

    Ok(())
  }

  async fn remove_agent(&self, hub: &str, agent: &str) -> Result<bool, Self::Error> {
    let mut nodes = self.nodes.lock().unwrap();
    let before = nodes.len();

    nodes.retain(|node| node.jake() != Some((hub, agent)));

    Ok(nodes.len() != before)
  }

  async fn exists(&self, name: &str) -> Result<bool, Self::Error> {
    Ok(self.nodes.lock().unwrap().iter().any(|node| node.name == name))
  }

  async fn record(&self, name: &str, check: &NodeCheck) -> Result<(), Self::Error> {
    let mut nodes = self.nodes.lock().unwrap();

    if let Some(node) = nodes.iter_mut().find(|node| node.name == name) {
      node.state = check.state;
      node.detail = check.detail.clone();
    }

    self.recorded.lock().unwrap().push((name.into(), check.clone()));

    Ok(())
  }
}

fn states(rows: &Rows) -> Vec<(String, NodeState)> {
  rows.nodes.lock().unwrap().iter().map(|node| (node.name.clone(), node.state)).collect()
}

#[tokio::test]
async fn a_whole_pass_asks_each_hub_once_stands_a_node_for_each_new_agent_and_says_whether_anything_moved()
{
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["agent_a", "alpha"]).await;
  let rows = Rows {
    hubs: vec![Hub {
      url: hub.url.clone(),
      token_file: "~/.local/share/jake/hub.env".into(),
    }],
    nodes: Mutex::new(vec![ssh_row("pc-node", None)]),
    ..Rows::default()
  };
  let machine = scratch.machine();

  assert_eq!(
    check_nodes(&rows, &machine, None).await.unwrap(),
    Checked {
      changed: true,
      checked: 3
    }
  );
  assert_eq!(hub.asked(), 1);
  assert_eq!(
    states(&rows),
    [
      ("pc-node".to_owned(), NodeState::Connected),
      ("agent-a".to_owned(), NodeState::Connected),
      ("alpha".to_owned(), NodeState::Connected),
    ]
  );
  assert_eq!(
    check_nodes(&rows, &machine, None).await.unwrap(),
    Checked {
      changed: false,
      checked: 3
    }
  );

  hub.answer(StatusCode::OK, attached(&["alpha"]));
  assert!(check_nodes(&rows, &machine, None).await.unwrap().changed);
  assert_eq!(states(&rows)[1], ("agent-a".to_owned(), NodeState::Disconnected));
}

#[tokio::test]
async fn an_agent_its_hub_says_has_departed_leaves_the_list_and_is_told_apart_when_checked_alone() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["alpha"]).await;
  let mut status = attached(&["alpha"]);
  status["departed"] = json!([{ "name": "agent_a", "departedAt": 1 }]);
  hub.answer(StatusCode::OK, status);
  let rows = Rows {
    hubs: vec![Hub {
      url: hub.url.clone(),
      token_file: "~/.local/share/jake/hub.env".into(),
    }],
    nodes: Mutex::new(vec![jake_row("agent-a", &hub.url, "agent_a"), jake_row("alpha", &hub.url, "alpha")]),
    ..Rows::default()
  };
  let machine = scratch.machine();

  assert_eq!(
    met(&machine, &jake_row("agent-a", &hub.url, "agent_a")).await,
    Met::new(NodeState::Disconnected, "the agent was uninstalled from its machine")
  );
  assert_eq!(
    check_nodes(&rows, &machine, None).await.unwrap(),
    Checked {
      changed: true,
      checked: 1
    }
  );
  assert_eq!(states(&rows), [("alpha".to_owned(), NodeState::Connected)]);
}

#[tokio::test]
async fn a_pass_for_one_node_checks_that_one_alone() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["agent_a", "alpha"]).await;
  let rows = Rows {
    hubs: vec![Hub {
      url: hub.url.clone(),
      token_file: "~/.local/share/jake/hub.env".into(),
    }],
    nodes: Mutex::new(vec![ssh_row("pc-node", None), jake_row("agent-a", &hub.url, "agent_a")]),
    ..Rows::default()
  };

  assert_eq!(
    check_nodes(&rows, &scratch.machine(), Some("agent-a")).await.unwrap(),
    Checked {
      changed: true,
      checked: 1
    }
  );
  assert_eq!(
    states(&rows),
    [("pc-node".to_owned(), NodeState::Unknown), ("agent-a".to_owned(), NodeState::Connected)]
  );
  assert!(scratch.logins().is_empty());
}

#[test]
fn a_row_lacking_a_field_its_kind_needs_describes_no_target() {
  let mut row = ssh_row("pc-node", None);

  row.host = None;
  assert!(row.target().is_none());
  assert!(jake_row("agent-a", "http://hub", "agent_a").target().is_some());
}

#[tokio::test]
async fn a_pass_over_node_s_own_store_adds_the_hub_s_agents_and_writes_every_check_to_its_row() {
  let scratch = Scratch::new();
  let hub = StoodHub::new(&["agent_a"]).await;
  let store = store::Store::in_memory().unwrap();

  store.remove_hub(store.hubs().unwrap()[0].id).unwrap();
  store.add_hub(&hub.url, "~/.local/share/jake/hub.env", None).unwrap();
  store
    .add_node("pc-node", None, None, store::nodes::SshLogin::new("192.0.2.82", "someone"))
    .unwrap();

  assert_eq!(
    check_nodes(&store, &scratch.machine(), None).await.unwrap(),
    Checked {
      changed: true,
      checked: 2
    }
  );

  let nodes = store.nodes().unwrap();
  let read: Vec<(&str, NodeState)> = nodes.iter().map(|node| (node.name.as_str(), node.state)).collect();

  assert_eq!(read, [("pc-node", NodeState::Connected), ("agent-a", NodeState::Connected)]);
  assert!(
    nodes
      .iter()
      .all(|node| node.checked_at.is_some() && node.connected_at == node.checked_at)
  );
  assert_eq!(node_check::check_row(&nodes[1]).jake(), Some((hub.url.as_str(), "agent_a")));
  assert_eq!(
    check_nodes(&store, &scratch.machine(), None).await.unwrap(),
    Checked {
      changed: false,
      checked: 2
    }
  );
}
