//! The hubs: listed once under an id of their own, read as a page, and removed with every node reached
//! through them.

use reqwest::Method;
use serde_json::json;

use crate::support::Stand;

#[tokio::test]
async fn a_hub_is_listed_once_and_removed_with_every_node_reached_through_it() {
  let stood = Stand::default().up().await;
  let seeded = stood.get("/hubs").await;

  assert_eq!((seeded["items"].as_array().unwrap().len(), seeded["more"].clone()), (1, json!(false)));

  let hub = stood
    .create(
      "/hubs",
      json!({ "address": "http://hub.example/", "tokenFile": "~/hub.env", "label": "Lab hub" }),
    )
    .await;

  assert_eq!(
    (hub["address"].clone(), hub["tokenFile"].clone(), hub["label"].clone()),
    (json!("http://hub.example"), json!("~/hub.env"), json!("Lab hub"))
  );
  assert_eq!(stood.get("/hubs").await["items"][1], hub);
  assert_eq!(
    stood
      .refused(
        Method::POST,
        "/hubs",
        Some(json!({ "address": "http://hub.example", "tokenFile": "~/x" }))
      )
      .await,
    "conflict"
  );
  assert_eq!(
    stood
      .refused(
        Method::POST,
        "/hubs",
        Some(json!({ "address": "ftp://hub.example", "tokenFile": "~/x" }))
      )
      .await,
    "bad-field"
  );
  stood.store().add_agent("http://hub.example", "box").unwrap();
  stood
    .store()
    .add_node("lab", None, None, store::nodes::SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let path = format!("/hubs/{}", hub["id"]);

  stood.delete(&path).await;
  assert_eq!(stood.refused(Method::DELETE, &path, None).await, "not-found");
  assert_eq!(stood.refused(Method::DELETE, "/hubs/first", None).await, "not-found");
  assert_eq!(
    stood.store().nodes().unwrap().iter().map(|node| node.name.as_str()).collect::<Vec<_>>(),
    ["lab"]
  );
}
