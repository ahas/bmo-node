//! The node list's rules the node registry's own tests held that the store's tests do not already: an
//! agent whose name is taken gets the next free one, a role on either connection type and refused where it
//! is not words, a Windows login kept, carried in its target and said beside its address, and a jake node
//! that takes a label and a role alone.

use node_server::views::target;
use policy::node_states::NodeState;
use serde_json::json;
use store::Store;
use store::nodes::{Checked, NodeChanges, SshLogin, agent_node_name};

fn home() -> SshLogin {
  SshLogin {
    port: 2222,
    key_path: Some("~/.ssh/id_home".into()),
    ..SshLogin::new("192.0.2.82", "someone")
  }
}

fn code(error: store::StoreError) -> &'static str {
  error.refusal().map(|refusal| refusal.code.as_str()).unwrap_or("not a refusal")
}

#[test]
fn an_agent_whose_name_is_taken_gets_the_next_free_one() {
  let store = Store::in_memory().unwrap();

  store.add_node("alpha", None, None, home()).unwrap();
  store.add_agent(store::HUB_SEED.0, "alpha").unwrap();
  store.add_agent(store::HUB_SEED.0, "ALPHA").unwrap();

  let names: Vec<String> = store.nodes().unwrap().into_iter().map(|node| node.name).collect();

  assert_eq!(names, ["alpha", "alpha-2", "alpha-3"]);
  assert_eq!(agent_node_name("node2.lab.local"), "node2-lab-local");
}

#[test]
fn a_role_reserves_a_node_for_one_service_or_several_on_either_connection_type() {
  let store = Store::in_memory().unwrap();

  assert_eq!(
    store.add_node("pc-node", None, Some("text".into()), home()).unwrap().role.as_deref(),
    Some("text")
  );
  store.add_agent(store::HUB_SEED.0, "agent_a").unwrap();
  assert_eq!(store.node("agent-a").unwrap().unwrap().role, None);

  let jake = store
    .update_node(
      "agent-a",
      NodeChanges {
        role: Some(Some("voice".into())),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!((jake.role.as_deref(), jake.label.as_str()), (Some("voice"), "agent_a"));
  assert_eq!(
    store
      .update_node(
        "pc-node",
        NodeChanges {
          role: Some(None),
          ..NodeChanges::default()
        }
      )
      .unwrap()
      .role,
    None
  );
  assert_eq!(
    store
      .update_node(
        "pc-node",
        NodeChanges {
          label: Some("home".into()),
          ..NodeChanges::default()
        }
      )
      .unwrap()
      .role,
    None
  );

  for refused in ["Two Words", "text+"] {
    let error = store
      .update_node(
        "pc-node",
        NodeChanges {
          role: Some(Some(refused.into())),
          ..NodeChanges::default()
        },
      )
      .unwrap_err();

    assert_eq!(code(error), "bad-field", "{refused}");
  }

  let both = store
    .update_node(
      "pc-node",
      NodeChanges {
        role: Some(Some("text+gate".into())),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!(both.services(), ["text", "gate"]);
}

#[test]
fn a_windows_login_is_kept_carried_in_its_target_said_beside_its_address_and_a_change_of_it_clears_the_check()
 {
  let store = Store::in_memory().unwrap();

  store.add_node("pc-node", None, None, home()).unwrap();
  store
    .record_check(
      "pc-node",
      &Checked {
        state: NodeState::Connected,
        detail: None,
        at: 100,
      },
    )
    .unwrap();

  let moved = store
    .update_node(
      "pc-node",
      NodeChanges {
        shell: Some("powershell".into()),
        user: Some("tester".into()),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!(
    (moved.shell.as_str(), moved.state, moved.connected_at),
    ("powershell", NodeState::Unknown, None)
  );
  assert_eq!(
    serde_json::to_value(target(&moved).unwrap()).unwrap(),
    json!({
      "kind": "ssh", "host": "192.0.2.82", "port": 2222, "user": "tester", "keyPath": "~/.ssh/id_home",
      "shell": "powershell", "role": null,
      "options": ["BatchMode=yes", "ConnectTimeout=5", "StrictHostKeyChecking=accept-new", "LogLevel=ERROR", "IdentitiesOnly=yes"],
    })
  );
  assert_eq!(moved.address(), "tester@192.0.2.82:2222 (PowerShell)");
  assert_eq!(
    code(
      store
        .update_node(
          "pc-node",
          NodeChanges {
            shell: Some("cmd".into()),
            ..NodeChanges::default()
          }
        )
        .unwrap_err()
    ),
    "bad-field"
  );

  store.add_agent(store::HUB_SEED.0, "agent_a").unwrap();
  assert_eq!(
    code(
      store
        .update_node(
          "agent-a",
          NodeChanges {
            shell: Some("powershell".into()),
            ..NodeChanges::default()
          }
        )
        .unwrap_err()
    ),
    "bad-field"
  );

  let relabelled = store
    .update_node(
      "agent-a",
      NodeChanges {
        label: Some("WSL".into()),
        ..NodeChanges::default()
      },
    )
    .unwrap();

  assert_eq!((relabelled.label.as_str(), relabelled.agent.as_deref()), ("WSL", Some("agent_a")));
  assert_eq!(
    code(
      store
        .update_node(
          "nobody",
          NodeChanges {
            label: Some("x".into()),
            ..NodeChanges::default()
          }
        )
        .unwrap_err()
    ),
    "no-node"
  );
  assert_eq!(code(store.add_node("blank", Some("  "), None, home()).unwrap_err()), "bad-field");
  assert_eq!(store.add_node("desk", None, None, home()).unwrap().label, "desk");
}
