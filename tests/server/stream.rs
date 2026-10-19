//! The machine's standing, the event stream and the health.

use std::time::Duration;

use bmo_node_client::NodeClient;
use bmo_node_client::events::{Event, EventKind, MachinePressure};
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
  let client = NodeClient::new(&stood.base, TOKEN).unwrap();
  // The stream answers once it has its subscription, so nothing published after this is missed.
  let mut events = Box::pin(client.events(&[EventKind::MachinePressure]).await.unwrap());
  let pressure = MachinePressure::Disk {
    free_gb: 3.0,
    limit_gb: 20.0,
    reading: None,
  };

  stood.service.events.nodes_changed();
  stood.service.events.publish(Event::MachinePressure(pressure.clone()));

  let first = tokio::time::timeout(Duration::from_secs(10), events.next()).await.unwrap().unwrap().unwrap();

  assert_eq!(first, Event::MachinePressure(pressure));
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
