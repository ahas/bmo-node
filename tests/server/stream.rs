//! The event stream and the health: each change published as its resource's own event, a stream narrowed
//! by kind and by machine and resumed after a number, a resume from before the window refused, the same
//! events as a page; and the loops once started making the service ready and publishing what they read.

use std::time::Duration;

use bmo_node_client::events::{Change, EventKind, EventsQuery};
use bmo_node_client::{ErrorCode, NodeClient};
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::json;

use crate::server::support::{Stand, TOKEN};

#[tokio::test]
async fn each_change_is_its_resource_s_own_event_and_a_stream_carries_the_kinds_it_names() {
  let stood = Stand::default().up().await;
  let client = NodeClient::new(&stood.base, TOKEN).unwrap();
  // The stream answers once it has its subscription, so nothing published after this is missed.
  let narrowed = EventsQuery {
    kinds: Some("node.deleted,machine.changed".into()),
    ..EventsQuery::default()
  };
  let mut events = Box::pin(client.events(&narrowed).await.unwrap());

  stood
    .create("/nodes", json!({ "name": "lab", "host": "192.0.2.4", "user": "someone" }))
    .await;
  stood.patch("/nodes/lab", json!({ "label": "Lab" })).await;
  stood.delete("/nodes/lab").await;
  stood.patch("/machines/local", json!({ "cpuLimit": 90 })).await;

  let deleted = tokio::time::timeout(Duration::from_secs(10), events.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  let machine = tokio::time::timeout(Duration::from_secs(10), events.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();

  assert_eq!((deleted.kind, deleted.data.clone()), (EventKind::NodeDeleted, json!({ "name": "lab" })));
  assert!(
    matches!(machine.change().unwrap(), Change::MachineChanged(changed) if changed.cpu_limit == Some(90.0))
  );
  assert!(machine.id > deleted.id);

  // The same events as a page, every kind, in the order they were published.
  let page = client.event_page(&EventsQuery::default()).await.unwrap();

  assert_eq!(
    page.items.iter().map(|event| event.kind).collect::<Vec<_>>(),
    [
      EventKind::NodeCreated,
      EventKind::NodeChanged,
      EventKind::NodeDeleted,
      EventKind::MachineChanged
    ]
  );
  assert!(matches!(page.items[1].change().unwrap(), Change::NodeChanged(node) if node.label == "Lab"));

  // A page after a number, narrowed to one machine's events.
  let after = EventsQuery {
    after: Some(page.items[0].id),
    machine_name: Some("local".into()),
    limit: Some(5),
    ..EventsQuery::default()
  };
  let narrowed = client.event_page(&after).await.unwrap();

  assert_eq!(
    (narrowed.items.len(), narrowed.items[0].kind, narrowed.more),
    (1, EventKind::MachineChanged, false)
  );
}

#[tokio::test]
async fn a_stream_resumes_after_the_last_event_read_and_one_from_before_the_window_is_gone() {
  let stood = Stand::default().up().await;
  let client = NodeClient::new(&stood.base, TOKEN).unwrap();

  stood
    .create("/nodes", json!({ "name": "lab", "host": "192.0.2.4", "user": "someone" }))
    .await;
  stood
    .create("/nodes", json!({ "name": "work", "host": "192.0.2.5", "user": "someone" }))
    .await;

  let kept = client.event_page(&EventsQuery::default()).await.unwrap().items;
  let resumed = EventsQuery {
    after: Some(kept[0].id),
    ..EventsQuery::default()
  };
  let mut events = Box::pin(client.events(&resumed).await.unwrap());
  let first = tokio::time::timeout(Duration::from_secs(10), events.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();

  assert_eq!(first, kept[1]);

  // A number an earlier run gave stands before this run's window.
  let gone = EventsQuery {
    after: Some(1),
    ..EventsQuery::default()
  };

  assert_eq!(
    client.event_page(&gone).await.unwrap_err().wire().map(|error| error.code),
    Some(ErrorCode::Gone)
  );

  let refused = client.events(&gone).await.err().unwrap();

  assert_eq!(refused.wire().map(|error| error.code), Some(ErrorCode::Gone));
  assert_eq!(stood.refused(Method::GET, "/events?kinds=node.moved", None).await, "bad-request");
}

#[tokio::test]
async fn the_health_names_its_version_and_why_it_is_not_ready() {
  let stood = Stand::default().up().await;

  assert_eq!(
    stood.get("/health").await,
    json!({ "ready": false, "reason": "the loops have not started", "version": bmo_node_plugin::VERSION })
  );

  stood.service.set_ready();
  assert_eq!(
    stood.get("/health").await,
    json!({ "ready": true, "reason": null, "version": bmo_node_plugin::VERSION })
  );
}

#[tokio::test]
async fn the_loops_once_started_make_the_service_ready_keep_the_standing_and_publish_each_reading() {
  let stood = Stand::default().up().await;
  let client = NodeClient::new(&stood.base, TOKEN).unwrap();
  let readings = EventsQuery {
    kinds: Some("reading.created".into()),
    machine_name: Some("local".into()),
    ..EventsQuery::default()
  };
  let mut events = Box::pin(client.events(&readings).await.unwrap());
  let loops = node_server::start(stood.service.clone()).unwrap();

  assert_eq!(stood.get("/health").await["ready"], true);

  let reading = tokio::time::timeout(Duration::from_secs(10), events.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();

  assert!(
    matches!(reading.change().unwrap(), Change::ReadingCreated(read) if read.span_ms == 1000 && read.machine_name == "local")
  );

  let kept = tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      let standing = stood.get("/machines/local").await["standing"].clone();

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
