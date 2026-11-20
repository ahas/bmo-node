//! The node list and the hubs: registering, changing and removing a node, the refusals, the agents a hub
//! adds and removes, a check written to its row, every node read with its hub, and whose each node is.

use policy::node_states::NodeState;
use policy::nodes::{NodeKind, Ownership, PersonalCallers};
use store::Store;
use store::nodes::{Checked, NodeChanges, SshLogin, agent_node_name};

use crate::store::scratch;

fn login(host: &str) -> SshLogin {
  SshLogin::new(host, "someone")
}

fn code(error: store::StoreError) -> &'static str {
  error.refusal().map(|refusal| refusal.code.as_str()).unwrap_or("not a refusal")
}

#[test]
fn an_ssh_node_is_registered_last_with_its_defaults() {
  let scratch = scratch();
  let store = &scratch.store;
  let first = store.add_node("lab", None, None, login("192.0.2.4")).unwrap();
  let second = store
    .add_node(
      "box",
      Some(" The box "),
      Some("song+voice".into()),
      SshLogin {
        port: 2222,
        key_path: Some("~/.ssh/node".into()),
        shell: "powershell".into(),
        ..login("box.example")
      },
    )
    .unwrap();

  assert_eq!(first.label, "lab");
  assert_eq!((first.kind, first.ssh_port(), first.shell.as_str()), (NodeKind::Ssh, 22, "posix"));
  assert_eq!(first.state, NodeState::Unknown);
  assert_eq!(first.credential(), None);
  assert_eq!(first.address(), "someone@192.0.2.4");
  assert_eq!(second.label, "The box");
  assert_eq!(second.services(), ["song", "voice"]);
  assert_eq!(second.address(), "someone@box.example:2222 (PowerShell)");
  assert_eq!(second.credential(), Some("~/.ssh/node"));
  assert!(second.position > first.position);
  assert_eq!(
    store.nodes().unwrap().iter().map(|node| node.name.as_str()).collect::<Vec<_>>(),
    ["lab", "box"]
  );
}

#[test]
fn what_the_table_does_not_take_is_refused_and_nothing_is_written() {
  let scratch = scratch();
  let store = &scratch.store;

  store.add_node("lab", None, None, login("192.0.2.4")).unwrap();

  assert_eq!(code(store.add_node("lab", None, None, login("192.0.2.5")).unwrap_err()), "conflict");
  assert_eq!(code(store.add_node("Lab_1", None, None, login("192.0.2.5")).unwrap_err()), "name-rule");
  assert_eq!(code(store.add_node(&"a".repeat(33), None, None, login("h")).unwrap_err()), "name-rule");
  for refused in [
    SshLogin {
      port: 0,
      ..login("h")
    },
    SshLogin {
      port: 65_536,
      ..login("h")
    },
    SshLogin {
      shell: "cmd".into(),
      ..login("h")
    },
    SshLogin {
      host: "two words".into(),
      ..login("h")
    },
    SshLogin {
      user: " ".into(),
      ..login("h")
    },
  ] {
    assert_eq!(code(store.add_node("other", None, None, refused).unwrap_err()), "bad-field");
  }
  assert_eq!(
    code(store.add_node("other", Some("two\nlines"), None, login("h")).unwrap_err()),
    "bad-field"
  );
  assert_eq!(
    code(store.add_node("other", Some(&"x".repeat(49)), None, login("h")).unwrap_err()),
    "bad-field"
  );
  assert_eq!(
    code(store.add_node("other", None, Some("Voice".into()), login("h")).unwrap_err()),
    "bad-field"
  );
  assert_eq!(store.nodes().unwrap().len(), 1);
}

#[test]
fn a_changed_login_forgets_the_last_check_and_a_new_label_does_not() {
  let scratch = scratch();
  let store = &scratch.store;
  let checked = Checked {
    state: NodeState::Connected,
    detail: None,
    at: 1_000,
  };

  store.add_node("lab", None, Some("voice".into()), login("192.0.2.4")).unwrap();
  store.record_check("lab", &checked).unwrap();

  let relabelled = store
    .update_node(
      "lab",
      NodeChanges {
        label: Some("Lab".into()),
        role: Some(None),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!((relabelled.label.as_str(), relabelled.role.as_deref()), ("Lab", None));
  assert_eq!((relabelled.state, relabelled.connected_at), (NodeState::Connected, Some(1_000)));

  let moved = store
    .update_node(
      "lab",
      NodeChanges {
        port: Some(2200),
        key_path: Some(Some("~/.ssh/lab".into())),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!((moved.ssh_port(), moved.key_path.as_deref()), (2200, Some("~/.ssh/lab")));
  assert_eq!((moved.state, moved.checked_at, moved.connected_at), (NodeState::Unknown, None, None));

  let cleared = store
    .update_node(
      "lab",
      NodeChanges {
        key_path: Some(None),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!(cleared.key_path, None);
  assert_eq!(code(store.update_node("gone", NodeChanges::default()).unwrap_err()), "not-found");
  assert_eq!(
    code(
      store
        .update_node(
          "lab",
          NodeChanges {
            port: Some(70_000),
            ..NodeChanges::default()
          }
        )
        .unwrap_err()
    ),
    "bad-field"
  );
}

#[test]
fn a_node_removed_is_gone_and_a_name_not_listed_is_no_removal() {
  let scratch = scratch();
  let store = &scratch.store;

  store.add_node("lab", None, None, login("192.0.2.4")).unwrap();

  assert!(store.node_exists("lab").unwrap());
  assert!(store.remove_node("lab").unwrap());
  assert!(!store.remove_node("lab").unwrap());
  assert_eq!(store.node("lab").unwrap(), None);
}

#[test]
fn an_attached_agent_becomes_a_hub_node_read_with_its_hub_once() {
  let scratch = scratch();
  let store = &scratch.store;
  let hub = store.add_hub("http://hub.example:8791/", "~/hub.env", Some("Lab hub")).unwrap();

  assert_eq!(hub.url, "http://hub.example:8791");
  assert_eq!(hub.label.as_deref(), Some("Lab hub"));
  assert!(store.add_agent(&hub.url, "Lab Box").unwrap());
  assert!(!store.add_agent(&hub.url, "Lab Box").unwrap());
  assert!(store.add_agent("http://other.example", "lab box").unwrap());

  let nodes = store.nodes().unwrap();
  let jake = &nodes[0];

  assert_eq!(
    (jake.name.as_str(), jake.label.as_str(), jake.kind),
    ("lab-box", "Lab Box", NodeKind::Hub)
  );
  assert_eq!(jake.hub_id, Some(hub.id));
  assert_eq!(jake.token_file.as_deref(), Some("~/hub.env"));
  assert_eq!(jake.credential(), Some("~/hub.env"));
  assert_eq!(jake.address(), "Lab Box @ http://hub.example:8791");
  assert_eq!(nodes[1].name, "lab-box-2");
  assert_eq!(
    (nodes[1].hub_id, nodes[1].token_file.as_deref()),
    (None, None),
    "a hub not listed has neither"
  );
  assert_eq!(
    code(
      store
        .update_node(
          "lab-box",
          NodeChanges {
            host: Some("h".into()),
            ..NodeChanges::default()
          }
        )
        .unwrap_err()
    ),
    "bad-field"
  );
  assert!(store.remove_agent(&hub.url, "Lab Box").unwrap());
  assert!(!store.remove_agent(&hub.url, "Lab Box").unwrap());
}

#[test]
fn an_agent_s_name_is_held_to_the_node_name_rule() {
  assert_eq!(agent_node_name("lab_box"), "lab-box");
  assert_eq!(agent_node_name("--Lab  Box!!"), "lab-box");
  assert_eq!(agent_node_name("!!!"), "jake");
  assert_eq!(agent_node_name(&"a".repeat(40)).len(), 32);
}

#[test]
fn a_hub_is_listed_once_and_removed_with_every_node_reached_through_it() {
  let scratch = scratch();
  let store = &scratch.store;

  let hub = store.add_hub("http://hub.example", "~/hub.env", None).unwrap();

  assert_eq!(code(store.add_hub("http://hub.example/", "~/other.env", None).unwrap_err()), "conflict");
  assert_eq!(code(store.add_hub("ftp://hub.example", "~/hub.env", None).unwrap_err()), "bad-field");
  assert_eq!(store.hub_at("http://hub.example/").unwrap().map(|found| found.id), Some(hub.id));
  store.add_agent("http://hub.example", "one").unwrap();
  store.add_agent("http://hub.example", "two").unwrap();
  store.add_node("lab", None, None, login("192.0.2.4")).unwrap();

  let removed = store.remove_hub(hub.id).unwrap().unwrap();

  assert_eq!(
    (removed.url.as_str(), removed.removed),
    ("http://hub.example", vec!["one".to_owned(), "two".to_owned()])
  );
  assert_eq!(store.remove_hub(hub.id).unwrap(), None);
  assert_eq!(store.nodes().unwrap().len(), 1);

  let again = store.add_hub("http://hub.example", "~/hub.env", None).unwrap();

  assert!(again.id > hub.id, "an id once used is never given again");
}

#[test]
fn a_check_is_written_and_the_last_connection_stands_until_another_moves_it() {
  let scratch = scratch();
  let store = &scratch.store;

  store.add_node("lab", None, None, login("192.0.2.4")).unwrap();
  store
    .record_check(
      "lab",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: 100,
      },
    )
    .unwrap();
  store
    .record_check(
      "lab",
      &Checked {
        state: NodeState::Unreachable,
        detail: Some("Connection timed out".into()),
        at: 200,
      },
    )
    .unwrap();
  store
    .record_check(
      "gone",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: 300,
      },
    )
    .unwrap();

  let node = store.node("lab").unwrap().unwrap();

  assert_eq!(node.state, NodeState::Unreachable);
  assert_eq!(node.detail.as_deref(), Some("Connection timed out"));
  assert_eq!((node.checked_at, node.connected_at), (Some(200), Some(100)));
  assert!(!store.node_exists("gone").unwrap());
}

/// Whose a node is, as the store holds it.
fn owned(node: &store::nodes::Node) -> (Ownership, Option<&str>) {
  (node.ownership, node.account.as_deref())
}

#[test]
fn a_person_s_machine_joins_personal_with_its_account_and_every_other_node_joins_shared() {
  let scratch = scratch();
  let store = &scratch.store;

  for agent in ["u60", "u1-cdd7da85", "u07", "u-laptop", "user5", "lab"] {
    store.add_agent("http://hub.example", agent).unwrap();
  }
  store.add_node("box", None, None, login("192.0.2.4")).unwrap();

  let nodes = store.nodes().unwrap();

  assert_eq!(
    nodes.iter().map(owned).collect::<Vec<_>>(),
    [
      (Ownership::Personal, Some("60")),
      (Ownership::Personal, Some("1")),
      (Ownership::Shared, None),
      (Ownership::Shared, None),
      (Ownership::Shared, None),
      (Ownership::Shared, None),
      (Ownership::Shared, None),
    ]
  );
}

#[test]
fn a_person_s_machine_joins_with_the_personal_callers_and_a_listed_node_keeps_its_own() {
  let dir = tempfile::tempdir().unwrap();
  let callers =
    PersonalCallers::try_from(vec!["mail-reader".to_owned(), "desk-driver".to_owned()]).unwrap();
  let store = Store::open_in(dir.path()).unwrap().with_personal_callers(callers);
  let named = |name: &str| store.node(name).unwrap().unwrap().callers;

  store.add_agent("http://hub.example", "u60").unwrap();
  store.add_agent("http://hub.example", "lab").unwrap();

  assert_eq!(named("u60").as_deref(), Some("desk-driver+mail-reader"));
  assert_eq!(named("lab"), None, "a machine of the platform joins naming none");

  store.set_callers("u60", None).unwrap();
  store.add_agent("http://hub.example", "u60").unwrap();

  assert_eq!(named("u60"), None, "a node already listed keeps what the owner wrote");
}

#[test]
fn a_node_s_ownership_is_changed_and_a_shared_node_holds_no_account() {
  let scratch = scratch();
  let store = &scratch.store;
  let change = |name: &str, ownership: Option<Ownership>, account: Option<Option<&str>>| {
    store.update_node(
      name,
      NodeChanges {
        ownership,
        account: account.map(|account| account.map(str::to_owned)),
        ..NodeChanges::default()
      },
    )
  };

  store.add_agent("http://hub.example", "u60").unwrap();
  store.add_node("box", None, None, login("192.0.2.4")).unwrap();

  assert_eq!(owned(&change("u60", Some(Ownership::Shared), None).unwrap()), (Ownership::Shared, None));
  assert_eq!(
    owned(&change("u60", Some(Ownership::Personal), Some(Some("61"))).unwrap()),
    (Ownership::Personal, Some("61"))
  );
  assert_eq!(
    owned(&change("box", Some(Ownership::Personal), None).unwrap()),
    (Ownership::Personal, None),
    "an ssh node takes it as a hub node does, and a person's PC need not name its account"
  );
  assert_eq!(owned(&change("box", None, Some(Some("7"))).unwrap()), (Ownership::Personal, Some("7")));
  assert_eq!(owned(&change("box", None, Some(None)).unwrap()), (Ownership::Personal, None));

  for (name, ownership, account) in [
    ("u60", Some(Ownership::Shared), Some(Some("61"))),
    ("u60", None, Some(Some("007"))),
    ("u60", None, Some(Some("sixty"))),
  ] {
    assert_eq!(code(change(name, ownership, account).unwrap_err()), "bad-field", "{account:?}");
  }
  assert_eq!(owned(&store.node("u60").unwrap().unwrap()), (Ownership::Personal, Some("61")));
}

#[test]
fn a_store_made_before_ownership_reads_its_hub_nodes_once() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("node.db");
  let store = Store::open(&path).unwrap();

  store.add_agent("http://hub.example", "u4").unwrap();
  store.add_agent("http://hub.example", "n2").unwrap();
  drop(store);

  let old = rusqlite::Connection::open(&path).unwrap();

  old
    .execute_batch(
      "ALTER TABLE nodes DROP COLUMN ownership; ALTER TABLE nodes DROP COLUMN account;
       PRAGMA user_version = 4;",
    )
    .unwrap();
  drop(old);

  let nodes = Store::open(&path).unwrap().nodes().unwrap();

  assert_eq!(
    nodes.iter().map(owned).collect::<Vec<_>>(),
    [(Ownership::Personal, Some("4")), (Ownership::Shared, None)]
  );
}
