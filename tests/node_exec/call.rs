//! A command run on a hub node through the tool: the hub asked for the node's own agent with the call's
//! shell, folder and variables, the answer its exit code and output in the shape the manifest declares, the
//! hub's token in no answer, and every road that fails said as what it is.

use bmo_node_client::nodes::{LoginShell, NodeKind};
use node_check::hub_exec::KEPT_BYTES;
use node_exec_tool::{Answer, ENTRY, answer};
use serde_json::{Value, json};

use super::{HUB_TOKEN, frame, handed, held_hub, hub_target, out, owned};
use crate::support::binary::built;

fn holds_to_the_output_schema(answer: &Answer) {
  let schema: Value = serde_json::from_str(include_str!("../../tools/node-exec/output.json")).unwrap();
  let structured = answer.structured.clone().expect("a command that ran answers structured content");

  assert!(jsonschema::is_valid(&schema, &structured), "{structured}");
}

#[tokio::test]
async fn the_owner_s_command_runs_on_the_node_s_agent_and_answers_its_code_and_output() {
  let stood = owned().await;

  *stood.world.frames.lock().unwrap() = vec![
    frame("started", json!({ "id": "c1" })),
    out("stdout", b"hello\n", 0),
    out("stderr", b"careful\n", 1),
    frame("exit", json!({ "code": 3, "signal": null, "canceled": false })),
  ];

  let arguments = json!({
    "node": "pc-four",
    "command": "Get-Date",
    "cwd": "C:\\work",
    "env": { "MODE": "quiet" },
  });
  let answered = answer(&handed("pc-agent", &[], arguments), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(
    answered.structured,
    Some(json!({
      "node": "pc-four",
      "shell": "powershell",
      "exitCode": 3,
      "signal": null,
      "stdout": "hello\n",
      "stderr": "careful\n",
      "stdoutCut": false,
      "stderrCut": false,
    }))
  );
  holds_to_the_output_schema(&answered);
  assert!(answered.text().starts_with("pc-four (powershell): exit code 3"), "{}", answered.text());

  let execs = stood.world.execs.lock().unwrap().clone();
  let rendered = serde_json::to_string(&answered).unwrap();

  assert_eq!(
    execs,
    [(
      Some("agent_w".into()),
      Some(format!("Bearer {HUB_TOKEN}")),
      json!({ "shell": "powershell", "command": "Get-Date", "cwd": "C:\\work", "env": { "MODE": "quiet" } })
    )]
  );
  assert!(!rendered.contains(HUB_TOKEN), "{rendered}");
}

#[tokio::test]
async fn a_stranger_is_refused_before_the_hub_is_asked() {
  let stood = owned().await;

  stood.world.supervisors.lock().unwrap().insert("other-agent".into(), json!(7));

  let answered = answer(
    &handed("other-agent", &[], json!({ "node": "pc-four", "command": "whoami" })),
    &stood.ground,
  )
  .await;

  assert!(answered.is_error);
  assert!(answered.text().starts_with("The node pc-four is account 4's;"), "{}", answered.text());
  assert!(stood.world.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_named_by_its_account_is_the_one_agency_s_record_names_for_it() {
  let stood = owned().await;

  *stood.world.frames.lock().unwrap() = vec![frame("exit", json!({ "code": 0 }))];

  let answered =
    answer(&handed("pc-agent", &[], json!({ "account": 4, "command": "hostname" })), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(answered.structured.unwrap()["node"], json!("pc-four"));
  assert_eq!(stood.world.execs.lock().unwrap()[0].0, Some("agent_w".into()));
}

#[tokio::test]
async fn an_account_with_no_node_is_refused_naming_it_before_the_hub_is_asked() {
  let stood = owned().await;

  let answered = answer(
    &handed("house", &["node.admin"], json!({ "account": "9", "command": "hostname" })),
    &stood.ground,
  )
  .await;

  assert!(answered.is_error);
  assert_eq!(
    answered.text(),
    "Account 9 has no node: no record of agency's names a person's node for it."
  );
  assert!(stood.world.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_account_s_node_is_judged_for_the_caller_as_a_node_named_is() {
  let stood = owned().await;

  stood.world.supervisors.lock().unwrap().insert("other-agent".into(), json!(7));

  let answered = answer(
    &handed("other-agent", &[], json!({ "account": "4", "command": "whoami" })),
    &stood.ground,
  )
  .await;

  assert!(answered.is_error);
  assert!(answered.text().starts_with("The node pc-four is account 4's;"), "{}", answered.text());
  assert!(stood.world.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_call_naming_both_the_node_and_the_account_or_neither_is_refused() {
  let stood = owned().await;
  let both = json!({ "node": "pc-four", "account": 4, "command": "true" });
  let neither = json!({ "command": "true" });

  for arguments in [both, neither] {
    let answered = answer(&handed("house", &["node.admin"], arguments), &stood.ground).await;

    assert_eq!(answered.text(), "Name the node by node or by account, one of the two.");
  }

  assert!(stood.world.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_administrator_names_the_shell_and_the_distribution_on_any_node() {
  let stood = owned().await;

  *stood.world.frames.lock().unwrap() = vec![frame("exit", json!({ "code": 0 }))];

  let arguments = json!({ "node": "pc-four", "command": "uname -a", "shell": "wsl", "distro": "Ubuntu" });
  let answered = answer(&handed("house", &["node.admin"], arguments), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(
    stood.world.execs.lock().unwrap()[0].2,
    json!({ "shell": "wsl", "command": "uname -a", "env": {}, "distro": "Ubuntu" })
  );
}

#[tokio::test]
async fn an_ssh_node_and_a_node_not_listed_are_refused_by_name() {
  let stood = owned().await;
  let mut ssh = hub_target("box", &stood.url, "none", LoginShell::Posix);

  ssh.kind = NodeKind::Ssh;
  stood.world.targets.lock().unwrap().insert("box".into(), ssh);

  let on_ssh = answer(
    &handed("house", &["node.admin"], json!({ "node": "box", "command": "true" })),
    &stood.ground,
  )
  .await;
  let unlisted = answer(
    &handed("house", &["node.admin"], json!({ "node": "gone", "command": "true" })),
    &stood.ground,
  )
  .await;

  assert!(on_ssh.text().starts_with("The node box is an ssh node"), "{}", on_ssh.text());
  assert!(unlisted.text().starts_with("the node gone could not be read"), "{}", unlisted.text());
  assert!(stood.world.execs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_hub_that_drops_the_run_midway_is_said_with_the_output_before_it() {
  let stood = owned().await;
  let frames = frame("started", json!({ "id": "c2" })) + &out("stdout", b"step one\n", 0);
  let hub = held_hub(frames).await;

  // The hub stops as soon as it has said the frames.
  drop(hub.stop);

  stood
    .world
    .targets
    .lock()
    .unwrap()
    .insert("pc-four".into(), hub_target("pc-four", &hub.url, "agent_w", LoginShell::Powershell));

  let answered =
    answer(&handed("pc-agent", &[], json!({ "node": "pc-four", "command": "long" })), &stood.ground).await;

  assert!(answered.is_error);
  assert_eq!(
    answered.text(),
    "The command on pc-four has no known end: the hub's stream broke off.\n--- stdout before that\nstep one\n"
  );
}

#[tokio::test]
async fn a_long_output_keeps_its_last_part_and_says_it_was_cut() {
  let stood = owned().await;
  let mut long = vec![b'a'; KEPT_BYTES];

  long.extend_from_slice(b"the end\n");
  *stood.world.frames.lock().unwrap() = vec![
    out("stdout", &long[..1000], 0),
    out("stdout", &long[1000..], 1),
    frame("exit", json!({ "code": 0 })),
  ];

  let answered = answer(
    &handed("pc-agent", &[], json!({ "node": "pc-four", "command": "spill" })),
    &stood.ground,
  )
  .await;
  let structured = answered.structured.clone().unwrap();
  let stdout = structured["stdout"].as_str().unwrap();

  assert_eq!(structured["stdoutCut"], json!(true));
  assert_eq!(stdout.len(), KEPT_BYTES);
  assert!(stdout.ends_with("the end\n"));
}

// The executable as the tool host starts it: the call on its standard input, one tool result on its standard
// output, and a clean exit; a call whose environment names no data root is answered why.
#[test]
fn the_executable_answers_one_call_on_its_standard_output() {
  use std::io::Write as _;
  use std::process::{Command, Stdio};

  let call = json!({
    "protocol": 1,
    "kind": "call",
    "entry": ENTRY,
    "arguments": { "node": "pc-four", "command": "hostname" },
    "session": { "holderName": "house", "namespaces": ["bmo"], "capabilities": [], "grants": [{ "resource": "node.admin" }] },
  });
  let mut child = Command::new(built("node-exec-tool", "node-exec-tool"))
    .env_remove("BMO_DATA_DIR")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
    .unwrap();

  child.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();

  let ended = child.wait_with_output().unwrap();
  let answered: Value = serde_json::from_slice(&ended.stdout).unwrap();

  assert!(ended.status.success());
  assert_eq!(answered["isError"], json!(true));
  assert_eq!(
    answered["content"][0]["text"],
    json!("the node service cannot be reached: the call's environment names no BMO_DATA_DIR")
  );
}
