//! The names behind the node list: a jake agent's node named by the node-name rule, and what names this
//! machine, an ssh node on it marked so and a jake node through a local hub not.

use node_server::this_machine::ThisMachine;
use node_server::views::is_this_machine;
use store::Store;
use store::nodes::{SshLogin, agent_node_name};

fn here() -> ThisMachine {
  ThisMachine::new(Some("host-a".into()), vec!["192.0.2.84".parse().unwrap(), "fe80::1".parse().unwrap()])
}

#[test]
fn an_agent_s_node_is_named_by_the_node_name_rule() {
  assert_eq!(agent_node_name("agent_a"), "agent-a");
  assert_eq!(agent_node_name("__Laptop  Home__"), "laptop-home");
  assert_eq!(agent_node_name("a_-b"), "a--b");
  assert_eq!(agent_node_name("___"), "jake");
  assert_eq!(agent_node_name(&"x".repeat(40)).len(), 32);
}

#[test]
fn a_loopback_address_localhost_the_host_name_and_an_own_address_name_this_machine() {
  for host in [
    "127.0.0.1",
    "127.0.1.1",
    "::1",
    "::ffff:127.0.0.1",
    "localhost",
    "HOST-A",
    "192.0.2.84",
    "FE80::1",
  ] {
    assert!(here().names(host), "{host}");
  }
}

#[test]
fn another_machine_s_address_or_name_does_not() {
  for host in ["192.0.2.82", "host-b", "10.0.0.1", "::2"] {
    assert!(!here().names(host), "{host}");
  }
}

#[test]
fn an_ssh_node_on_this_machine_is_marked_so_and_a_jake_node_through_a_local_hub_is_not() {
  let store = Store::in_memory().unwrap();

  store.add_node("here", None, None, SshLogin::new("127.0.0.1", "tester")).unwrap();
  store.add_node("far", None, None, SshLogin::new("192.0.2.82", "tester")).unwrap();
  store.add_agent(store::HUB_SEED.0, "lab").unwrap();

  let marked: Vec<(String, bool)> = store
    .nodes()
    .unwrap()
    .iter()
    .map(|node| (node.name.clone(), is_this_machine(node, &here())))
    .collect();

  assert_eq!(
    marked,
    [("here".to_owned(), true), ("far".to_owned(), false), ("lab".to_owned(), false)]
  );
}
