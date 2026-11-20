//! The node policy's files read as their types: every node state declared with its tone on a shared and on
//! a personal node, the logins'
//! options and how a login's end is read, the Windows sensor setup, what a node's name and kind may be, and
//! which nodes the hardware monitor reads.

use policy::node_states::{NODE_STATES, NodeState, Ownership, StateTone, Tone};
use policy::nodes::{MonitoredAccounts, NodeKind, is_node_name};
use policy::ssh::SSH;
use policy::windows_sensors::WINDOWS_SENSORS;

#[test]
fn the_file_declares_every_state_and_no_other_each_with_its_tone() {
  assert_eq!(NODE_STATES.keys().copied().collect::<Vec<_>>(), NodeState::ALL.to_vec());
  assert_eq!(
    [NodeState::Connected.tone(), NodeState::Auth.tone(), NodeState::Unknown.tone()],
    [Tone::Good, Tone::Bad, Tone::Unknown]
  );
  for state in [NodeState::Disconnected, NodeState::Hostkey, NodeState::Unreachable, NodeState::Error] {
    assert_eq!(state.tone(), Tone::Bad, "{}", state.name());
  }
}

#[test]
fn a_person_s_pc_switched_off_reads_unknown_and_a_failed_login_still_reads_bad() {
  let on_personal = |state: NodeState| state.tone_on(Ownership::Personal);

  assert_eq!(
    NodeState::ALL.map(on_personal),
    [Tone::Good, Tone::Unknown, Tone::Bad, Tone::Bad, Tone::Unknown, Tone::Bad, Tone::Unknown]
  );
  for state in NodeState::ALL {
    assert_eq!(state.tone_on(Ownership::Shared), state.tone(), "{}", state.name());
  }
}

#[test]
fn the_monitor_reads_every_shared_node_and_the_personal_nodes_of_the_accounts_named_alone() {
  let monitored = MonitoredAccounts::try_from(vec!["1".to_owned()]).unwrap();

  assert_eq!(
    [
      monitored.read(Ownership::Shared, None),
      monitored.read(Ownership::Personal, Some("1")),
      monitored.read(Ownership::Personal, Some("60")),
      monitored.read(Ownership::Personal, None),
      MonitoredAccounts::default().read(Ownership::Personal, Some("1")),
      MonitoredAccounts::default().read(Ownership::Shared, None),
    ],
    [true, true, false, false, false, true]
  );
  assert_eq!(
    MonitoredAccounts::try_from(vec!["1".to_owned(), "01".to_owned()]),
    Err("a monitored account is an account's id as a decimal number, not \"01\"".to_owned())
  );
}

#[test]
fn a_column_reads_as_its_state_and_anything_else_as_unknown() {
  assert_eq!(NodeState::of_column("hostkey"), NodeState::Hostkey);
  assert_eq!(NodeState::of_column("lost"), NodeState::Unknown);
  assert_eq!(NodeState::of_code("lost"), None);
  for state in NodeState::ALL {
    assert_eq!(NodeState::of_code(state.name()), Some(state));
  }
}

#[test]
fn a_login_nobody_is_at_asks_nobody_and_the_check_runs_nothing() {
  assert!(SSH.batch.options.iter().any(|one| one == "BatchMode=yes"));
  assert!(!SSH.person.options.iter().any(|one| one == "BatchMode=yes"));
  assert_eq!(SSH.check.command, "exit");
}

#[test]
fn openssh_s_own_words_decide_what_a_failed_login_met() {
  assert_eq!(
    SSH.ended_state("someone@192.0.2.10: Permission denied (publickey)."),
    Some(NodeState::Auth)
  );
  assert_eq!(
    SSH.ended_state("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@"),
    Some(NodeState::Hostkey)
  );
  assert_eq!(
    SSH.ended_state("ssh: connect to host 198.51.100.9 port 22: Connection timed out"),
    Some(NodeState::Unreachable)
  );
  assert_eq!(SSH.ended_state("ssh: Could not resolve hostname nowhere"), Some(NodeState::Unreachable));
  assert_eq!(SSH.ended_state("something odd"), None);
}

#[test]
fn the_windows_sensor_setup_names_its_place_release_and_driver() {
  assert!(!WINDOWS_SENSORS.library_dir.is_empty());
  assert!(WINDOWS_SENSORS.release.starts_with('v'));
  assert!(!WINDOWS_SENSORS.driver_package.is_empty());
}

#[test]
fn a_node_is_reached_over_ssh_or_through_a_hub_and_nothing_else() {
  assert_eq!(NodeKind::of_code("ssh"), Some(NodeKind::Ssh));
  assert_eq!(NodeKind::of_code("hub"), Some(NodeKind::Hub));
  assert_eq!(NodeKind::of_code("jake"), None);
  for kind in NodeKind::ALL {
    assert_eq!(NodeKind::of_code(kind.name()), Some(kind));
  }
}

#[test]
fn a_node_s_name_is_lowercase_letters_digits_and_hyphens_of_at_most_32() {
  for name in ["lab-box", "n2", "0", &"a".repeat(32)] {
    assert!(is_node_name(name), "{name}");
  }
  for name in ["", "-lead", "Lab", "lab_box", "lab box", &"a".repeat(33)] {
    assert!(!is_node_name(name), "{name}");
  }
}
