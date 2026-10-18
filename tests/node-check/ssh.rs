//! An ssh node's login as the policy composes it, and what a login that ended met.

use node_check::rows::SshTarget;
use node_check::ssh::{SshRun, login_args, read_run};
use node_check::{Machine, Met, NodeState};
use policy::ssh::SSH;

fn target() -> SshTarget {
  SshTarget {
    host: "192.0.2.82".into(),
    port: 2222,
    user: "someone".into(),
    key_path: Some("~/.ssh/id_home".into()),
  }
}

#[test]
fn logs_in_in_batch_mode_with_the_node_s_own_key_and_port_running_nothing() {
  let machine = Machine::new("ssh", "/home/user");
  let args = login_args(&machine, &SSH.batch, &target(), Some(&SSH.check.command));
  let line = args.join(" ");

  assert!(line.starts_with("-o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new"));
  assert!(line.contains("-o IdentitiesOnly=yes -i /home/user/.ssh/id_home"));
  assert_eq!(args[args.len() - 4..], ["-p", "2222", "someone@192.0.2.82", "exit"]);

  let person = login_args(&machine, &SSH.person, &target(), None).join(" ");

  assert_eq!(
    person,
    "-o StrictHostKeyChecking=accept-new -i /home/user/.ssh/id_home -p 2222 someone@192.0.2.82"
  );
}

#[test]
fn reads_what_the_login_met() {
  let read = |code: Option<i32>, stderr: &str| {
    read_run(&SshRun {
      code,
      stderr: stderr.into(),
    })
  };

  assert_eq!(read(Some(0), "").state, NodeState::Connected);
  assert_eq!(
    read(Some(255), "someone@192.0.2.82: Permission denied (publickey,password).\n").state,
    NodeState::Auth
  );
  assert_eq!(read(Some(255), "Host key verification failed.\n").state, NodeState::Hostkey);
  assert_eq!(
    read(Some(255), "ssh: connect to host 192.0.2.82 port 22: Connection timed out\n").state,
    NodeState::Unreachable
  );
  assert_eq!(
    read(Some(255), "ssh: Could not resolve hostname nowhere: Name or service not known\n").state,
    NodeState::Unreachable
  );
  assert_eq!(read(None, "").state, NodeState::Unreachable);
  assert_eq!(read(Some(1), "something odd\n"), Met::new(NodeState::Error, "something odd"));
  assert_eq!(read(Some(3), ""), Met::new(NodeState::Error, "ssh ended with 3"));
}
