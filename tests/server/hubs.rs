//! The hubs: listed once under an id of their own, read as a page, and removed with every node reached
//! through them.

use reqwest::Method;
use serde_json::json;

use crate::server::support::Stand;

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

// A hub node removed with a leave is removed only once its hub took the leave: a hub that cannot be told leaves the
// node listed and says why (docs/nodes.md, 「Registering and changing a node」).
#[tokio::test]
async fn a_hub_node_removed_with_a_leave_stays_where_its_hub_cannot_take_the_leave() {
  let stood = Stand::default().up().await;

  stood
    .create("/hubs", json!({ "address": "http://127.0.0.1:9", "tokenFile": "~/no-such-hub.env" }))
    .await;
  stood.store().add_agent("http://127.0.0.1:9", "box").unwrap();

  let named = stood
    .store()
    .nodes()
    .unwrap()
    .into_iter()
    .find(|node| node.agent.as_deref() == Some("box"))
    .map(|node| node.name)
    .expect("the agent stands as a hub node");

  assert_eq!(
    stood.refused(Method::DELETE, &format!("/nodes/{named}?leave=true"), None).await,
    "conflict"
  );
  assert!(stood.store().nodes().unwrap().iter().any(|node| node.name == named));

  // Removed without a leave, it is removed alone.
  stood.delete(&format!("/nodes/{named}")).await;
  assert!(!stood.store().nodes().unwrap().iter().any(|node| node.name == named));
}
