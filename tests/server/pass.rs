//! The node pass: every node checked, and the screens told only when a state or the list moved.

use std::time::Duration;

use bmo_node_client::events::EventKind;
use store::nodes::SshLogin;
use tokio::sync::broadcast::error::TryRecvError;

use crate::support::Stand;

#[tokio::test]
async fn the_node_pass_checks_every_node_and_tells_the_screens_only_when_a_state_moved() {
  let stood = Stand::default().up().await;
  let mut told = stood.service.events.subscribe();
  let mut count = || {
    let mut seen = 0;

    loop {
      match told.try_recv() {
        Ok(event) if event.kind() == EventKind::NodesChanged => seen += 1,
        Ok(_) => {}
        Err(TryRecvError::Empty | TryRecvError::Closed) => return seen,
        Err(TryRecvError::Lagged(_)) => {}
      }
    }
  };

  // The seeded hub answers nothing, so its pass moves nothing.
  node_server::node_pass(&stood.service).await.unwrap();
  assert_eq!(count(), 0);

  stood
    .store()
    .add_node(
      "pc-node",
      Some("PC"),
      None,
      SshLogin {
        port: 2222,
        key_path: Some("/nowhere/id_home".into()),
        ..SshLogin::new("192.0.2.1", "tester")
      },
    )
    .unwrap();
  assert!(node_server::node_pass(&stood.service).await.unwrap());
  assert_eq!(count(), 1);
  assert_eq!(
    stood.store().node("pc-node").unwrap().unwrap().detail.as_deref(),
    Some("no key at /nowhere/id_home")
  );

  node_server::node_pass(&stood.service).await.unwrap();
  tokio::time::sleep(Duration::from_millis(10)).await;
  assert_eq!(count(), 0);
}
