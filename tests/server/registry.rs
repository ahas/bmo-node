//! The registry's routes: an ssh node added, changed and removed, a hub listed and removed with its nodes,
//! and the connection a tool reaches a node by.

use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::support::{Stand, refused};

#[tokio::test]
async fn an_ssh_node_is_added_changed_and_removed_and_what_the_list_does_not_take_is_refused() {
  let stood = Stand::default().up().await;
  let added = stood
    .post(
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
  assert_eq!(
    refused(&stood.post("/nodes", json!({ "name": "lab", "host": "h", "user": "u" })).await),
    "name-taken"
  );
  assert_eq!(
    refused(&stood.post("/nodes", json!({ "name": "Lab_1", "host": "h", "user": "u" })).await),
    "name-rule"
  );
  assert_eq!(
    refused(
      &stood
        .post("/nodes", json!({ "name": "other", "host": "h", "user": "u", "port": 0 }))
        .await
    ),
    "bad-field"
  );

  let (status, answer) = stood.ask(Method::POST, "/nodes", Some(json!({ "name": "other" }))).await;

  assert_eq!((status, answer["code"].clone()), (StatusCode::BAD_REQUEST, json!("bad-request")));

  let (_, set) = stood
    .ask(
      Method::PATCH,
      "/nodes/lab",
      Some(json!({ "label": "Lab", "keyPath": null, "role": null })),
    )
    .await;

  assert_eq!(
    (set["label"].clone(), set["credential"].clone(), set["role"].clone()),
    (json!("Lab"), json!(null), json!(null))
  );

  let (status, _) = stood.ask(Method::PATCH, "/nodes/lab", Some(json!({ "port": "many" }))).await;

  assert_eq!(status, StatusCode::BAD_REQUEST);

  let (_, gone) = stood.ask(Method::PATCH, "/nodes/gone", Some(json!({ "label": "x" }))).await;

  assert_eq!(refused(&gone), "no-node");

  let (_, removed) = stood.ask(Method::DELETE, "/nodes/lab", None).await;

  assert_eq!(removed, json!({ "removed": "lab" }));

  let (_, again) = stood.ask(Method::DELETE, "/nodes/lab", None).await;

  assert_eq!(refused(&again), "no-node");
}

#[tokio::test]
async fn a_hub_is_listed_once_and_removed_with_every_node_reached_through_it() {
  let stood = Stand::default().up().await;

  assert_eq!(
    stood
      .post("/hubs", json!({ "url": "http://hub.example/", "tokenFile": "~/hub.env" }))
      .await,
    json!({ "url": "http://hub.example", "tokenFile": "~/hub.env" })
  );
  assert_eq!(
    refused(&stood.post("/hubs", json!({ "url": "http://hub.example", "tokenFile": "~/x" })).await),
    "hub-taken"
  );
  stood.store().add_agent("http://hub.example", "box").unwrap();

  let (_, removed) = stood.ask(Method::DELETE, "/hubs?url=http://hub.example", None).await;

  assert_eq!(removed, json!({ "url": "http://hub.example", "removed": ["box"] }));

  let (_, again) = stood.ask(Method::DELETE, "/hubs?url=http://hub.example", None).await;

  assert_eq!(refused(&again), "no-hub");
  assert!(stood.store().nodes().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_s_target_is_its_connection_without_a_credential_and_a_row_missing_a_field_has_none() {
  let stood = Stand::default().up().await;

  stood
    .post(
      "/nodes",
      json!({ "name": "lab", "host": "192.0.2.4", "user": "someone", "role": "voice" }),
    )
    .await;
  stood
    .post("/hubs", json!({ "url": "http://hub.example", "tokenFile": "~/hub.env" }))
    .await;
  stood.store().add_agent("http://hub.example", "box").unwrap();
  stood.store().add_agent("http://unlisted.example", "far").unwrap();

  assert_eq!(
    stood.get("/nodes/lab/target").await,
    json!({ "kind": "ssh", "host": "192.0.2.4", "port": 22, "user": "someone", "keyPath": null, "shell": "posix", "role": "voice" })
  );
  assert_eq!(
    stood.get("/nodes/box/target").await,
    json!({ "kind": "jake", "hubUrl": "http://hub.example", "tokenFile": "~/hub.env", "agent": "box", "role": null })
  );
  assert_eq!(refused(&stood.get("/nodes/far/target").await), "incomplete");
  assert_eq!(refused(&stood.get("/nodes/gone/target").await), "no-node");
}
