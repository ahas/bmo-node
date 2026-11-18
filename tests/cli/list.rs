//! The node verbs run against a store: a node registered, changed, reserved and removed, a hub listed and
//! removed with a jake node checked through it, and the list saying where each node is reached, where its
//! credential is kept, its state and when it was last connected.

use clap::Parser;
use node_check::Machine;
use node_server::cli::{NodesArgs, clock, format_hub_list, format_node_list, nodes};
use node_server::views::target;
use policy::node_states::NodeState;
use serde_json::{Value, json};
use store::Store;
use store::nodes::{Checked, SshLogin};

fn args(words: &[&str]) -> NodesArgs {
  NodesArgs::try_parse_from(std::iter::once("node").chain(words.iter().copied())).unwrap()
}

/// A store and a machine of the test's own, whose home holds nothing.
fn stand() -> (tempfile::TempDir, Store, Machine) {
  let dir = tempfile::tempdir().unwrap();
  let store = Store::in_memory().unwrap();
  let machine = Machine::new("ssh", dir.path());

  (dir, store, machine)
}

#[tokio::test]
async fn registers_changes_and_removes_a_node_and_refuses_what_the_table_refuses() {
  let (_dir, store, machine) = stand();
  let run = |words: &'static [&'static str]| nodes(&store, &machine, args(words));

  let added = run(&[
    "--add",
    "pc-node",
    "--ssh",
    "someone@192.0.2.82",
    "--port",
    "2222",
    "--label",
    "home pc",
  ])
  .await
  .unwrap();

  assert!(added[0].contains("pc-node [ssh] home pc — someone@192.0.2.82:2222"), "{added:?}");
  assert!(
    run(&["--add", "pc-node", "--ssh", "someone@x"])
      .await
      .unwrap_err()
      .contains("already exists")
  );
  run(&["--set", "pc-node", "--label", "main home pc"]).await.unwrap();
  assert_eq!(store.node("pc-node").unwrap().unwrap().label, "main home pc");

  // A role reserves the node for one service, and a tool reaching the node is told it beside the connection.
  let reserved = run(&["--set", "pc-node", "--role", "voice"]).await.unwrap();

  assert!(reserved[0].contains(", voice only"));

  let target: Value = serde_json::from_str(&run(&["--target", "pc-node"]).await.unwrap()[0]).unwrap();

  assert_eq!(
    target,
    json!({
      "nodeName": "pc-node", "kind": "ssh", "role": "voice",
      "host": "192.0.2.82", "port": 2222, "user": "someone", "keyPath": null, "shell": "posix",
      "sshOptions": ["BatchMode=yes", "StrictHostKeyChecking=accept-new", "LogLevel=ERROR", "ServerAliveInterval=15",
        "ServerAliveCountMax=3"],
      "sshArgs": [
        "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=accept-new", "-o", "LogLevel=ERROR",
        "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
        "-p", "2222", "someone@192.0.2.82",
      ],
      "hubId": null, "hubAddress": null, "hubTokenFile": null, "agentName": null,
    })
  );
  run(&["--set", "pc-node", "--role", "none"]).await.unwrap();

  let target: Value = serde_json::from_str(&run(&["--target", "pc-node"]).await.unwrap()[0]).unwrap();

  assert_eq!(target["role"], Value::Null);
  assert_eq!(run(&["--remove", "pc-node"]).await.unwrap(), ["removed pc-node"]);
  assert_eq!(run(&["--remove", "pc-node"]).await.unwrap_err(), "no node named pc-node");
  assert!(run(&["--set", "x"]).await.unwrap_err().contains("usage: bmo-node --config <file> node"));
  assert!(
    run(&["--add", "Bad_Name", "--ssh", "u@h"])
      .await
      .unwrap_err()
      .contains("a node's name is")
  );
  assert!(
    run(&["--add", "box", "--ssh", "u@h", "--role", "Not+ok"])
      .await
      .unwrap_err()
      .contains("a role names")
  );
  assert!(
    run(&["--add", "box", "--ssh", "u@h", "--port", "70000"])
      .await
      .unwrap_err()
      .contains("1 to 65535")
  );
}

#[tokio::test]
async fn lists_and_removes_a_hub_and_checks_a_hub_node_now_through_it_printing_no_token() {
  let (dir, store, machine) = stand();
  let token_file = dir.path().join("hub.env");
  let token = token_file.to_string_lossy().into_owned();

  std::fs::write(&token_file, "JAKE_CONTROL_TOKEN=do-not-print-me\n").unwrap();

  // A hub address nothing listens on, so the check meets a refused connection at once.
  let listed = nodes(&store, &machine, args(&["--add-hub", "http://[::1]:1", "--token-file", &token]))
    .await
    .unwrap();

  assert_eq!(listed, [format!("hub 2 http://[::1]:1, token file {token}")]);
  assert!(
    nodes(&store, &machine, args(&["--add-hub", "http://[::1]:1", "--token-file", &token]))
      .await
      .unwrap_err()
      .contains("already listed")
  );
  store.add_agent("http://[::1]:1", "far_box").unwrap();

  let checked = nodes(&store, &machine, args(&["--check", "far-box"])).await.unwrap();

  assert!(checked[0].contains("unreachable"), "{checked:?}");
  assert!(!checked.concat().contains("do-not-print-me"));
  assert_eq!(store.node("far-box").unwrap().unwrap().state, NodeState::Unreachable);
  assert!(nodes(&store, &machine, args(&[])).await.unwrap()[0].contains("hub 2 http://[::1]:1"));
  assert!(
    nodes(&store, &machine, args(&["--remove-hub", "http://[::1]:1"])).await.unwrap()[0]
      .contains("removed the hub http://[::1]:1")
  );
  assert!(store.node("far-box").unwrap().is_none());
  assert_eq!(
    nodes(&store, &machine, args(&["--check", "gone"])).await.unwrap_err(),
    "no node named gone"
  );
}

#[tokio::test]
async fn says_each_node_s_address_where_its_credential_is_kept_its_state_and_when_it_was_last_connected() {
  let (_dir, store, machine) = stand();

  store.add_agent(store::HUB_SEED.0, "agent_a").unwrap();
  store.add_agent(store::HUB_SEED.0, "u60").unwrap();
  store
    .add_node(
      "pc-node",
      Some("home pc"),
      None,
      SshLogin {
        port: 2222,
        ..SshLogin::new("192.0.2.82", "someone")
      },
    )
    .unwrap();
  store
    .record_check(
      "pc-node",
      &Checked {
        state: NodeState::Auth,
        detail: Some("Permission denied (publickey,password).".into()),
        at: 1_790_000_000,
      },
    )
    .unwrap();

  let listed = store.nodes().unwrap();
  let text = format_node_list(&listed);

  assert!(text.contains(
    "agent-a [hub] agent_a — agent_a @ http://127.0.0.1:8791, token file ~/.local/share/jake/hub.env"
  ));
  assert_eq!(
    format_hub_list(&store.hubs().unwrap()),
    "hub 1 http://127.0.0.1:8791, token file ~/.local/share/jake/hub.env"
  );
  assert_eq!(format_hub_list(&[]), "(no hubs)");
  assert!(text.contains(
    "u60 [hub] u60 — u60 @ http://127.0.0.1:8791, token file ~/.local/share/jake/hub.env, personal to account 60\n"
  ));
  assert!(text.contains("pc-node [ssh] home pc — someone@192.0.2.82:2222, key (ssh config)\n"));
  assert!(text.contains("    auth · checked 2026-09-21 14:13:20 UTC · last connected -"), "{text}");
  assert_eq!(clock(Some(1_790_000_000)), "2026-09-21 14:13:20 UTC");
  assert!(text.contains("    Permission denied (publickey,password)."));
  assert_eq!(format_node_list(&[]), "(no nodes)");
  assert_eq!(
    serde_json::to_value(target(&listed[0], &machine).unwrap()).unwrap(),
    json!({
      "nodeName": "agent-a", "kind": "hub", "role": null,
      "host": null, "port": null, "user": null, "keyPath": null, "shell": "posix", "sshOptions": [],
      "sshArgs": [],
      "hubId": 1, "hubAddress": "http://127.0.0.1:8791", "hubTokenFile": "~/.local/share/jake/hub.env",
      "agentName": "agent_a",
    })
  );
}
