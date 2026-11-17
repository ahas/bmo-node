//! The nodes and the node checks: the token every route needs, the list paged in its order, an ssh node
//! added, changed and removed, the target a node is reached by, and the check made now.

use policy::node_states::NodeState;
use reqwest::{Method, StatusCode};
use serde_json::json;
use store::nodes::{Checked, SshLogin};

use crate::server::support::Stand;

#[tokio::test]
async fn every_route_needs_the_token_and_a_path_none_answers_is_no_route() {
  let stood = Stand::default().up().await;

  for token in [None, Some("wrong")] {
    let (status, answer) = stood.ask_with(token, Method::GET, "/nodes", None).await;

    assert_eq!((status, answer["code"].clone()), (StatusCode::UNAUTHORIZED, json!("unauthorized")));
  }

  assert_eq!(stood.refused(Method::GET, "/nowhere", None).await, "no-route");
  assert_eq!(stood.refused(Method::POST, "/nodes/check", Some(json!({}))).await, "no-route");
}

#[tokio::test]
async fn the_list_comes_in_its_order_paged_after_a_name_and_each_node_says_how_it_is_reached() {
  let stood = Stand::default().up().await;
  let store = stood.store();
  let now = store::now_seconds();

  store.add_node("here", None, None, SshLogin::new("127.0.0.1", "someone")).unwrap();
  store
    .add_node(
      "work",
      Some("Work"),
      Some("voice".into()),
      SshLogin {
        port: 2200,
        ..SshLogin::new("192.0.2.5", "someone")
      },
    )
    .unwrap();
  store.add_hub("http://127.0.0.1:9999", "~/hub.env", None).unwrap();
  store.add_agent("http://127.0.0.1:9999", "box").unwrap();
  store
    .record_check(
      "work",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: now - 30,
      },
    )
    .unwrap();

  let whole = stood.get("/nodes").await;

  assert_eq!(whole["more"], false);
  assert_eq!(
    whole["items"][1],
    json!({
      "name": "work", "label": "Work", "kind": "ssh", "hubId": null, "agentName": null,
      "address": "someone@192.0.2.5:2200", "credential": null, "role": "voice", "callers": [],
      "state": "connected",
      "tone": "good", "detail": null, "checkedAt": (now - 30) * 1000, "connectedAt": (now - 30) * 1000,
      "up": true, "thisMachine": false,
    })
  );
  assert_eq!(whole["items"][0]["thisMachine"], true);
  assert_eq!(
    (
      whole["items"][2]["kind"].clone(),
      whole["items"][2]["address"].clone(),
      whole["items"][2]["credential"].clone()
    ),
    (json!("hub"), json!("box @ http://127.0.0.1:9999"), json!("~/hub.env"))
  );
  assert_eq!(whole["items"][2]["hubId"], 2);

  let first = stood.get("/nodes?limit=1").await;
  let next = stood.get("/nodes?after=here&limit=1").await;

  assert_eq!((first["items"][0]["name"].clone(), first["more"].clone()), (json!("here"), json!(true)));
  assert_eq!((next["items"][0]["name"].clone(), next["more"].clone()), (json!("work"), json!(true)));
  assert_eq!(stood.get("/nodes?after=box").await, json!({ "items": [], "more": false }));
  assert_eq!(stood.refused(Method::GET, "/nodes?after=gone", None).await, "bad-request");
  assert_eq!(stood.get("/nodes/work").await, whole["items"][1]);
  assert_eq!(stood.refused(Method::GET, "/nodes/gone", None).await, "not-found");
}

#[tokio::test]
async fn an_ssh_node_is_added_changed_and_removed_and_what_the_list_does_not_take_is_refused() {
  let stood = Stand::default().up().await;
  let added = stood
    .create(
      "/nodes",
      json!({ "name": "lab", "host": "192.0.2.4", "user": "someone", "port": 2222, "keyPath": "~/.ssh/lab", "role": "voice" }),
    )
    .await;

  assert_eq!(
    (
      added["name"].clone(),
      added["address"].clone(),
      added["credential"].clone(),
      added["role"].clone()
    ),
    (json!("lab"), json!("someone@192.0.2.4:2222"), json!("~/.ssh/lab"), json!("voice"))
  );

  let taken = json!({ "name": "lab", "host": "h", "user": "u" });

  assert_eq!(stood.refused(Method::POST, "/nodes", Some(taken)).await, "conflict");
  assert_eq!(
    stood
      .refused(Method::POST, "/nodes", Some(json!({ "name": "Lab_1", "host": "h", "user": "u" })))
      .await,
    "name-rule"
  );
  assert_eq!(
    stood
      .refused(
        Method::POST,
        "/nodes",
        Some(json!({ "name": "other", "host": "h", "user": "u", "port": 0 }))
      )
      .await,
    "bad-field"
  );
  assert_eq!(
    stood.refused(Method::POST, "/nodes", Some(json!({ "name": "other" }))).await,
    "bad-request"
  );

  let changed = stood
    .patch("/nodes/lab", json!({ "label": "Lab", "keyPath": null, "role": null }))
    .await;

  assert_eq!(
    (changed["label"].clone(), changed["credential"].clone(), changed["role"].clone()),
    (json!("Lab"), json!(null), json!(null))
  );
  assert_eq!(
    stood.refused(Method::PATCH, "/nodes/lab", Some(json!({ "port": "many" }))).await,
    "bad-request"
  );
  assert_eq!(
    stood.refused(Method::PATCH, "/nodes/gone", Some(json!({ "label": "x" }))).await,
    "not-found"
  );

  stood.delete("/nodes/lab").await;
  assert_eq!(stood.refused(Method::DELETE, "/nodes/lab", None).await, "not-found");
}

#[tokio::test]
async fn a_node_s_target_is_how_it_is_reached_without_a_credential_and_a_row_missing_a_field_has_none() {
  let stood = Stand::default().up().await;

  stood
    .create(
      "/nodes",
      json!({ "name": "lab", "host": "192.0.2.4", "user": "someone", "port": 2222, "keyPath": "~/.ssh/lab", "role": "voice" }),
    )
    .await;

  let hub = stood
    .create("/hubs", json!({ "address": "http://hub.example", "tokenFile": "~/hub.env" }))
    .await;

  stood.store().add_agent("http://hub.example", "box").unwrap();
  stood.store().add_agent("http://unlisted.example", "far").unwrap();

  let lab = stood.get("/nodes/lab/target").await;
  let line = lab["sshArgs"]
    .as_array()
    .unwrap()
    .iter()
    .map(|one| one.as_str().unwrap())
    .collect::<Vec<_>>()
    .join(" ");

  assert_eq!(
    (
      lab["kind"].clone(),
      lab["host"].clone(),
      lab["port"].clone(),
      lab["shell"].clone(),
      lab["role"].clone()
    ),
    (json!("ssh"), json!("192.0.2.4"), json!(2222), json!("posix"), json!("voice"))
  );
  assert!(lab["sshOptions"].as_array().unwrap().contains(&json!("BatchMode=yes")));
  assert!(line.starts_with("-o BatchMode=yes"));
  assert!(line.contains(&format!("-o IdentitiesOnly=yes -i {}/.ssh/lab", stood.home().display())));
  assert!(line.ends_with("-p 2222 someone@192.0.2.4"));
  assert_eq!(
    stood.get("/nodes/box/target").await,
    json!({
      "nodeName": "box", "kind": "hub", "role": null, "host": null, "port": null, "user": null, "keyPath": null,
      "shell": "posix", "sshOptions": [], "sshArgs": [], "hubId": hub["id"], "hubAddress": "http://hub.example",
      "hubTokenFile": "~/hub.env", "agentName": "box",
    })
  );

  stood.store().set_agent_shell("http://hub.example", "box", "powershell").unwrap();
  assert_eq!(stood.get("/nodes/box/target").await["shell"], json!("powershell"));
  assert_eq!(stood.refused(Method::GET, "/nodes/far/target", None).await, "incomplete");
  assert_eq!(stood.refused(Method::GET, "/nodes/gone/target", None).await, "not-found");
}

#[tokio::test]
async fn a_check_made_now_writes_each_named_node_s_state_and_answers_them_as_they_stand() {
  let stood = Stand::default().up().await;
  let store = stood.store();

  store.add_node("lab", None, None, SshLogin::new("192.0.2.4", "someone")).unwrap();
  store.add_node("work", None, None, SshLogin::new("192.0.2.5", "someone")).unwrap();

  let checked = stood.create("/node-checks", json!({ "nodeNames": ["lab"] })).await;
  let items = checked["items"].as_array().unwrap();

  assert_eq!(items.len(), 1);
  assert_eq!(
    (
      items[0]["name"].clone(),
      items[0]["state"].clone(),
      items[0]["tone"].clone(),
      items[0]["up"].clone()
    ),
    (json!("lab"), json!("connected"), json!("good"), json!(true))
  );
  assert_eq!(store.node("work").unwrap().unwrap().state, NodeState::Unknown);

  let every = stood.create("/node-checks", json!({})).await;

  assert_eq!(every["items"].as_array().unwrap().len(), 2);
  assert_eq!(store.node("work").unwrap().unwrap().state, NodeState::Connected);
  assert_eq!(
    stood
      .refused(Method::POST, "/node-checks", Some(json!({ "nodeNames": ["gone"] })))
      .await,
    "bad-field"
  );
  assert_eq!(
    stood.refused(Method::POST, "/node-checks", Some(json!({ "nodeNames": "lab" }))).await,
    "bad-request"
  );
}
