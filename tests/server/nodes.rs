//! Job's node reads and the console's node list: the rows in job's shape, an ssh node's reach, the views
//! with whether a node is this machine and takes jobs, and the check made now.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Method;
use serde_json::json;
use store::nodes::SshLogin;

use crate::support::{Stand, TOKEN, refused};

#[tokio::test]
async fn the_node_rows_and_an_ssh_node_s_reach_come_in_job_s_fields() {
  let stood = Stand::default().up().await;
  let store = stood.store();

  store
    .add_node(
      "lab",
      Some("Lab"),
      Some("jobs".into()),
      SshLogin {
        port: 2222,
        key_path: Some("~/.ssh/lab".into()),
        shell: "powershell".into(),
        ..SshLogin::new("192.0.2.4", "someone")
      },
    )
    .unwrap();
  store.add_agent("http://hub.example", "box").unwrap();

  let rows = stood.get("/nodes").await;

  assert_eq!(
    rows["nodes"][0],
    json!({ "name": "lab", "label": "Lab", "kind": "ssh", "role": "jobs", "state": "unknown", "checkedAt": null })
  );
  assert_eq!(rows["nodes"][1]["kind"], "jake");
  assert_eq!(
    stood.get("/nodes/lab/reach").await,
    json!({ "host": "192.0.2.4", "port": 2222, "user": "someone", "keyPath": "~/.ssh/lab", "shell": "powershell" })
  );
  assert_eq!(refused(&stood.get("/nodes/box/reach").await), "not-ssh");
  assert_eq!(refused(&stood.get("/nodes/gone/reach").await), "no-node");
}

#[tokio::test]
async fn the_console_s_views_say_where_each_node_is_reached_whether_it_is_this_machine_and_takes_jobs() {
  let stood = Stand::default().up().await;
  let store = stood.store();

  store.add_node("here", None, None, SshLogin::new("127.0.0.1", "someone")).unwrap();
  store
    .add_node("lab", None, Some("voice".into()), SshLogin::new("192.0.2.4", "someone"))
    .unwrap();
  store
    .add_node(
      "work",
      None,
      None,
      SshLogin {
        port: 2200,
        ..SshLogin::new("192.0.2.5", "someone")
      },
    )
    .unwrap();
  store.add_hub("http://127.0.0.1:9999", "~/hub.env").unwrap();
  store.add_agent("http://127.0.0.1:9999", "box").unwrap();

  let views = stood.get("/nodes/view").await;
  let view = |at: usize| views["nodes"][at].clone();

  assert_eq!(
    (view(0)["thisMachine"].clone(), view(0)["takesJobs"].clone()),
    (json!(true), json!(false))
  );
  assert_eq!(
    (view(1)["thisMachine"].clone(), view(1)["takesJobs"].clone()),
    (json!(false), json!(false))
  );
  assert_eq!(
    view(2),
    json!({
      "name": "work", "label": "work", "kind": "ssh", "agent": null, "address": "someone@192.0.2.5:2200",
      "credential": null, "role": null, "state": "unknown", "tone": "unknown", "detail": null, "checkedAt": null,
      "connectedAt": null, "thisMachine": false, "takesJobs": true,
    })
  );
  assert_eq!(
    (
      view(3)["address"].clone(),
      view(3)["credential"].clone(),
      view(3)["thisMachine"].clone()
    ),
    (json!("box @ http://127.0.0.1:9999"), json!("~/hub.env"), json!(false))
  );
}

#[tokio::test]
async fn a_check_made_now_writes_each_node_s_state_and_tells_the_screens() {
  let stood = Stand::default().up().await;
  let events = stood
    .client
    .get(format!("{}/events?kinds=nodes-changed", stood.base))
    .bearer_auth(TOKEN)
    .send()
    .await
    .unwrap();
  let mut events = events.bytes_stream();

  stood
    .store()
    .add_node("lab", None, None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let checked = stood.post("/nodes/check", json!({ "name": "lab" })).await;

  assert_eq!(
    (checked["nodes"][0]["state"].clone(), checked["nodes"][0]["tone"].clone()),
    (json!("connected"), json!("good"))
  );

  let told = tokio::time::timeout(Duration::from_secs(10), async {
    let mut text = String::new();

    while let Some(chunk) = events.next().await {
      text.push_str(&String::from_utf8_lossy(&chunk.unwrap()));

      if text.contains("event: nodes-changed") {
        return text;
      }
    }

    text
  })
  .await
  .unwrap();

  assert!(told.contains("event: nodes-changed"));

  let (status, answer) = stood.ask(Method::POST, "/nodes/check", Some(json!({ "name": 3 }))).await;

  assert_eq!((status.as_u16(), answer["code"].clone()), (400, json!("bad-request")));
}
