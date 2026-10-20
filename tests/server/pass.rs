//! The node pass: every node checked, and a node's event published only where the node moved, its check's
//! times aside, whatever moved it.

use bmo_node_client::events::EventKind;
use store::nodes::SshLogin;

use crate::support::Stand;

#[tokio::test]
async fn the_node_pass_checks_every_node_and_publishes_only_what_moved() {
  let stood = Stand::default().up().await;
  let events = &stood.service.events;
  let kinds = |after: i64| -> Vec<EventKind> {
    events.after(after).unwrap().into_iter().map(|published| published.event.kind).collect()
  };
  let start = events.floor();

  // The seeded hub answers nothing and no node stands, so the pass moves nothing.
  node_server::node_pass(&stood.service).await.unwrap();
  assert_eq!(kinds(start), []);

  // A node the command added outside the service is published by the next pass, with its check.
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
  node_server::node_pass(&stood.service).await.unwrap();
  assert_eq!(kinds(start), [EventKind::NodeCreated]);
  assert_eq!(
    stood.store().node("pc-node").unwrap().unwrap().detail.as_deref(),
    Some("no key at /nowhere/id_home")
  );

  // A pass that finds the same again moves only the check's time, which publishes nothing.
  let seen = events.after(start).unwrap().last().unwrap().event.id;

  node_server::node_pass(&stood.service).await.unwrap();
  assert_eq!(kinds(seen), []);

  stood.store().remove_node("pc-node").unwrap();
  node_server::node_pass(&stood.service).await.unwrap();
  assert_eq!(kinds(seen), [EventKind::NodeDeleted]);
}
