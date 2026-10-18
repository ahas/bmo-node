//! The machine's standing, the event stream and the health.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::json;

use crate::support::{Stand, TOKEN};

#[tokio::test]
async fn the_standing_is_empty_until_the_minute_look_keeps_one() {
  let stood = Stand::default().up().await;

  assert_eq!(
    stood.get("/machine/standing").await,
    json!({ "reading": null, "held": [], "browsers": 0, "places": [], "largest": [] })
  );

  node_server::machine_look(&stood.service).unwrap();

  let standing = stood.get("/machine/standing").await;

  assert_eq!(standing["reading"]["threads"], 24);
  assert!(
    standing["held"]
      .as_array()
      .unwrap()
      .iter()
      .any(|one| one == "load 74 >= 24 threads x 2")
  );
}

#[tokio::test]
async fn a_stream_hands_on_the_kinds_it_names_and_only_those() {
  let stood = Stand::default().up().await;
  let answer = stood
    .client
    .get(format!("{}/events?kinds=machine-pressure", stood.base))
    .bearer_auth(TOKEN)
    .send()
    .await
    .unwrap();

  assert_eq!(answer.headers()["content-type"], "text/event-stream");

  let mut events = answer.bytes_stream();

  // Give the stream its subscription before anything is published.
  tokio::time::sleep(Duration::from_millis(200)).await;
  stood.service.events.publish("nodes-changed", json!({}));
  stood
    .service
    .events
    .publish("machine-pressure", json!({ "cause": "disk", "freeGb": 3 }));

  let text = tokio::time::timeout(Duration::from_secs(10), async {
    let mut text = String::new();

    while let Some(chunk) = events.next().await {
      text.push_str(&String::from_utf8_lossy(&chunk.unwrap()));

      if text.contains("freeGb") {
        break;
      }
    }

    text
  })
  .await
  .unwrap();

  assert!(text.contains("event: machine-pressure"));
  assert!(text.contains(r#"data: {"cause":"disk","freeGb":3}"#));
  assert!(!text.contains("nodes-changed"));
}

#[tokio::test]
async fn the_health_names_the_process_its_version_and_whether_it_is_ready() {
  let stood = Stand::default().up().await;
  let health = stood.get("/health").await;

  assert_eq!(health["pid"], std::process::id());
  assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
  assert_eq!(health["ready"], false);

  stood.service.set_ready();
  assert_eq!(stood.get("/health").await["ready"], true);
}

#[tokio::test]
async fn the_loops_once_started_make_the_service_ready_and_keep_this_machine_s_standing() {
  let stood = Stand::default().up().await;
  let loops = node_server::start(stood.service.clone()).unwrap();

  assert_eq!(stood.get("/health").await["ready"], true);

  let kept = tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      let standing = stood.get("/machine/standing").await;

      if !standing["reading"].is_null() {
        return standing;
      }

      tokio::time::sleep(Duration::from_millis(50)).await;
    }
  })
  .await
  .unwrap();

  assert_eq!(kept["reading"]["threads"], 24);

  for task in loops {
    task.abort();
  }
}
