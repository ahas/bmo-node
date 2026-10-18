//! Agency's four calls, each read back into the shapes agency's node peer deserializes: this machine's
//! room, a node's link, the node rows and a node's batch login.

use policy::node_states::NodeState;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use store::nodes::{Checked, SshLogin};

use crate::support::{Stand, refused};

/// The shapes agency reads (agency's `docs/api.md`, 「The node service」).
#[derive(Deserialize)]
struct Room {
  held: Vec<String>,
}

#[derive(Deserialize)]
struct LinkState {
  up: bool,
}

#[derive(Deserialize)]
struct NodeRow {
  node: String,
  #[serde(default)]
  label: Option<String>,
  state: String,
  #[serde(default)]
  detail: Option<String>,
}

#[derive(Deserialize)]
struct NodeRows {
  nodes: Vec<NodeRow>,
}

#[derive(Deserialize)]
struct Login {
  ssh: Option<Vec<String>>,
}

fn read<T: for<'a> Deserialize<'a>>(answer: Value) -> T {
  serde_json::from_value(answer).unwrap()
}

#[tokio::test]
async fn every_route_needs_the_token_and_a_path_none_answers_is_no_route() {
  let stood = Stand::default().up().await;

  for token in [None, Some("wrong")] {
    let (status, answer) = stood.ask_with(token, Method::POST, "/nodes/list", Some(json!({}))).await;

    assert_eq!((status, answer["code"].clone()), (StatusCode::UNAUTHORIZED, json!("unauthorized")));
  }

  let (status, answer) = stood.ask(Method::GET, "/nowhere", None).await;

  assert_eq!((status, answer["code"].clone()), (StatusCode::NOT_FOUND, json!("no-route")));

  let (status, answer) = stood.ask(Method::POST, "/links/state", Some(json!({ "nobody": 1 }))).await;

  assert_eq!((status, answer["code"].clone()), (StatusCode::BAD_REQUEST, json!("bad-request")));
  assert!(answer["error"].is_string());
}

#[tokio::test]
async fn the_machine_s_room_names_each_hardware_limit_it_meets() {
  let stood = Stand::default().up().await;

  // The disk line held under any disk the test runs on, so only the written figures hold.
  stood.store().set_setting("machine_limit:launch_disk_free_gb", "0.000001").unwrap();

  let room: Room = read(stood.post("/machine/room", json!({})).await);

  assert_eq!(
    room.held,
    ["memory pressure 25% >= 5%", "available memory 2GB < 4GB", "load 74 >= 24 threads x 2"]
  );
}

#[tokio::test]
async fn a_node_s_link_is_up_where_its_last_check_found_it_connected_lately() {
  let stood = Stand::default().up().await;
  let store = stood.store();
  let now = store::now_seconds();

  store.add_node("lab", None, None, SshLogin::new("192.0.2.4", "someone")).unwrap();
  store.add_node("old", None, None, SshLogin::new("192.0.2.5", "someone")).unwrap();
  store.add_node("never", None, None, SshLogin::new("192.0.2.6", "someone")).unwrap();
  store
    .record_check(
      "lab",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: now - 30,
      },
    )
    .unwrap();
  store
    .record_check(
      "old",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: now - 600,
      },
    )
    .unwrap();

  for (node, up) in [("lab", true), ("old", false), ("never", false), ("gone", false)] {
    let link: LinkState = read(stood.post("/links/state", json!({ "node": node })).await);

    assert_eq!(link.up, up, "{node}");
  }
}

#[tokio::test]
async fn the_node_rows_come_in_the_list_s_order_with_their_kind() {
  let stood = Stand::default().up().await;
  let store = stood.store();

  store.add_node("lab", Some("Lab"), None, SshLogin::new("192.0.2.4", "someone")).unwrap();
  store.add_hub("http://hub.example", "~/hub.env").unwrap();
  store.add_agent("http://hub.example", "box").unwrap();
  store
    .record_check(
      "lab",
      &Checked {
        state: NodeState::Unreachable,
        detail: Some("timed out".into()),
        at: 1,
      },
    )
    .unwrap();

  let answer = stood.post("/nodes/list", json!({})).await;

  assert_eq!(answer["nodes"][0]["kind"], "ssh");
  assert_eq!(answer["nodes"][1]["kind"], "jake");

  let rows: NodeRows = read(answer);
  let read: Vec<_> = rows
    .nodes
    .iter()
    .map(|row| (row.node.as_str(), row.label.as_deref(), row.state.as_str(), row.detail.as_deref()))
    .collect();

  assert_eq!(
    read,
    [
      ("lab", Some("Lab"), "unreachable", Some("timed out")),
      ("box", Some("box"), "unknown", None)
    ]
  );
}

#[tokio::test]
async fn a_node_s_login_is_its_batch_login_ahead_of_the_command_or_none_without_an_ssh_route() {
  let stood = Stand::default().up().await;
  let store = stood.store();

  store
    .add_node(
      "lab",
      None,
      None,
      SshLogin {
        port: 2222,
        key_path: Some("~/.ssh/lab".into()),
        ..SshLogin::new("192.0.2.4", "someone")
      },
    )
    .unwrap();
  store.add_agent("http://hub.example", "box").unwrap();

  let login: Login = read(stood.post("/nodes/login", json!({ "node": "lab" })).await);
  let line = login.ssh.unwrap().join(" ");

  assert!(line.starts_with("-o BatchMode=yes"));
  assert!(line.contains(&format!("-o IdentitiesOnly=yes -i {}/.ssh/lab", stood.home().display())));
  assert!(line.ends_with("-p 2222 someone@192.0.2.4"));

  let login: Login = read(stood.post("/nodes/login", json!({ "node": "box" })).await);

  assert!(login.ssh.is_none());
  assert_eq!(refused(&stood.post("/nodes/login", json!({ "node": "gone" })).await), "no-node");
}
