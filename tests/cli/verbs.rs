//! The node verbs read from their arguments: nothing lists, each verb reads its own fields, and what makes
//! no sense is refused before anything is written.

use clap::Parser;
use node_server::cli::{NodeCommand, NodesArgs};
use policy::nodes::Ownership;
use store::nodes::{NodeChanges, SshLogin};

fn command(args: &[&str]) -> Result<NodeCommand, String> {
  let args = NodesArgs::try_parse_from(std::iter::once("node").chain(args.iter().copied()))
    .map_err(|why| why.to_string())?;

  args.command()
}

fn login(host: &str, user: &str, port: i64) -> SshLogin {
  SshLogin {
    port,
    ..SshLogin::new(host, user)
  }
}

#[test]
fn nothing_lists_and_each_verb_reads_its_own_fields() {
  assert_eq!(command(&[]), Ok(NodeCommand::List));
  assert_eq!(
    command(&[
      "--add",
      "pc-node",
      "--ssh",
      "someone@192.0.2.82",
      "--port",
      "2222",
      "--key",
      "~/.ssh/id_home",
      "--label",
      "home pc"
    ]),
    Ok(NodeCommand::Add {
      name: "pc-node".into(),
      label: Some("home pc".into()),
      role: None,
      login: SshLogin {
        key_path: Some("~/.ssh/id_home".into()),
        ..login("192.0.2.82", "someone", 2222)
      },
    })
  );
  assert!(matches!(
    command(&["--add", "gpu-box", "--ssh", "me@box", "--role", "text"]),
    Ok(NodeCommand::Add { role: Some(role), .. }) if role == "text"
  ));
  assert_eq!(
    command(&["--set", "n2", "--role", "none"]),
    Ok(NodeCommand::Set {
      name: "n2".into(),
      changes: NodeChanges {
        role: Some(None),
        ..NodeChanges::default()
      },
      callers: None,
    })
  );
  assert_eq!(
    command(&["--set", "pc-four", "--callers", "desk-driver"]),
    Ok(NodeCommand::Set {
      name: "pc-four".into(),
      changes: NodeChanges::default(),
      callers: Some(Some("desk-driver".into())),
    })
  );
  assert_eq!(
    command(&["--set", "u60", "--ownership", "personal", "--account", "61"]),
    Ok(NodeCommand::Set {
      name: "u60".into(),
      changes: NodeChanges {
        ownership: Some(Ownership::Personal),
        account: Some(Some("61".into())),
        ..NodeChanges::default()
      },
      callers: None,
    })
  );
  assert_eq!(
    command(&["--set", "u60", "--account", "none"]),
    Ok(NodeCommand::Set {
      name: "u60".into(),
      changes: NodeChanges {
        account: Some(None),
        ..NodeChanges::default()
      },
      callers: None,
    })
  );
  assert_eq!(
    command(&["--add-hub", "http://127.0.0.1:8791", "--token-file", "~/hub.env"]),
    Ok(NodeCommand::AddHub {
      url: "http://127.0.0.1:8791".into(),
      token_file: "~/hub.env".into(),
    })
  );
  assert_eq!(
    command(&["--set", "pc-node", "--key", "none", "--port", "22"]),
    Ok(NodeCommand::Set {
      name: "pc-node".into(),
      changes: NodeChanges {
        key_path: Some(None),
        port: Some(22),
        ..NodeChanges::default()
      },
      callers: None,
    })
  );
  assert_eq!(command(&["--remove-hub", "http://h"]), Ok(NodeCommand::RemoveHub("http://h".into())));
  assert_eq!(command(&["--remove", "pc-node"]), Ok(NodeCommand::Remove("pc-node".into())));
  assert_eq!(command(&["--check"]), Ok(NodeCommand::Check(None)));
  assert_eq!(command(&["--check", "pc-node"]), Ok(NodeCommand::Check(Some("pc-node".into()))));
  assert_eq!(command(&["--target", "pc-node"]), Ok(NodeCommand::Target("pc-node".into())));
  assert!(matches!(
    command(&["--add", "win", "--ssh", "me@win", "--shell", "powershell"]),
    Ok(NodeCommand::Add { login, .. }) if login.shell == "powershell"
  ));
}

#[test]
fn an_ssh_node_added_with_no_port_and_no_key_takes_22_and_the_account_s_own_ssh_setup() {
  assert!(matches!(
    command(&["--add", "box", "--ssh", "me@box"]),
    Ok(NodeCommand::Add { login: added, .. }) if added == login("box", "me", 22)
  ));
}

#[test]
fn refuses_what_it_cannot_make_sense_of() {
  for args in [
    &["--add", "x"][..],
    &["--add", "x", "--jake", "http://h"],
    &["--add", "x", "--ssh", "a@b", "--token-file", "f"],
    &["--add-hub", "http://h"],
    &["--add-hub", "http://h", "--token-file", "f", "--port", "22"],
    &["--add-hub", "http://h", "--token-file", "f", "--remove", "x"],
    &["--add", "x", "--ssh", "a@b", "--port", "twenty"],
    &["--add", "x", "--remove", "y"],
    &["--set", "x"],
    &["--label", "x"],
    &["--add"],
    &["--add", "x", "--ssh", "a@b", "--bogus", "1"],
    &["--add", "x", "--ssh", "a@b", "--ssh", "c@d"],
    &["--target"],
    &["--target", "x", "--label", "y"],
    &["--set", "x", "--shell", "cmd"],
    &["--set", "x", "--ownership", "everyone"],
    &["--add", "x", "--ssh", "a@b", "--ownership", "personal"],
  ] {
    assert!(command(args).is_err(), "{args:?} was read");
  }
}
