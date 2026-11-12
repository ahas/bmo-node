//! The service stopped as the signal stops it (docs/api.md, 「Stopping」): no caller taken after, the event
//! streams ended, a call under way let finish, and one outliving the drain cut.

use std::time::Duration;

use bmo_node_client::NodeClient;
use bmo_node_client::events::EventsQuery;
use futures_util::StreamExt;
use node_server::stop::Drained;
use serde_json::json;
use store::nodes::SshLogin;

use crate::server::support::{Stand, Stood, TOKEN};

/// How long a test waits on what should come at once before it fails, so a fault fails rather than hangs.
const BOUND: Duration = Duration::from_secs(10);

/// A gated service with one ssh node, its check started and its login waiting at the gate.
async fn checking(drain: Duration) -> (Stood, tokio::task::JoinHandle<serde_json::Value>, std::fs::File) {
  let stood = Stand {
    ssh_gated: true,
    drain,
    ..Stand::default()
  }
  .up()
  .await;

  stood
    .store()
    .add_node("lab", None, None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let (base, client) = (stood.base.clone(), stood.client.clone());
  let check = tokio::spawn(async move {
    let answer = client
      .post(format!("{base}/node-checks"))
      .bearer_auth(TOKEN)
      .json(&json!({ "nodeNames": ["lab"] }))
      .send()
      .await
      .unwrap();

    assert!(answer.status().is_success(), "{}", answer.status());
    answer.json().await.unwrap()
  });
  let gate = tokio::time::timeout(BOUND, stood.login_at_gate()).await.unwrap();

  (stood, check, gate)
}

/// Whether the service's address refuses a new connection, asked until it does or the bound runs out.
async fn refuses(base: &str) -> bool {
  let address = base.trim_start_matches("http://").to_owned();

  for _ in 0..1_000 {
    if tokio::net::TcpStream::connect(&address).await.is_err() {
      return true;
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  false
}

#[tokio::test]
async fn a_stop_refuses_new_callers_ends_the_streams_and_lets_a_call_under_way_finish() {
  let (mut stood, check, gate) = checking(Duration::from_secs(600)).await;
  let client = NodeClient::new(&stood.base, TOKEN).unwrap();
  let mut events = Box::pin(client.events(&EventsQuery::default()).await.unwrap());

  stood.stop();

  assert!(refuses(&stood.base).await, "the service still takes callers after the stop");
  assert!(
    tokio::time::timeout(BOUND, events.next()).await.unwrap().is_none(),
    "the stream stayed open"
  );
  assert!(
    !check.is_finished() && !stood.served.is_finished(),
    "the stop did not wait for the call"
  );

  drop(gate);

  let checked = tokio::time::timeout(BOUND, check).await.unwrap().unwrap();

  assert_eq!(
    (checked["items"][0]["name"].clone(), checked["items"][0]["state"].clone()),
    (json!("lab"), json!("connected"))
  );
  assert_eq!(
    tokio::time::timeout(BOUND, &mut stood.served).await.unwrap().unwrap().unwrap(),
    Drained::Finished
  );
}

#[tokio::test]
async fn a_call_still_under_way_when_the_drain_runs_out_is_cut() {
  let (mut stood, check, gate) = checking(Duration::from_millis(50)).await;

  stood.stop();

  assert_eq!(
    tokio::time::timeout(BOUND, &mut stood.served).await.unwrap().unwrap().unwrap(),
    Drained::Cut
  );

  // The login let end, so nothing it started outlives the test.
  drop(gate);
  let _ = tokio::time::timeout(BOUND, check).await;
}
