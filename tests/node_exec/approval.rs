//! A command that changes a machine runs only on the owner's approval (docs/tools.md, 「A command that changes a
//! machine」): asked as a question first, refused until the owner himself chose to run that very command.

use node_exec_tool::answer;
use node_exec_tool::approval::{RUN, SKIP, asked_text, changes_machine};
use serde_json::json;

use super::{frame, handed, owned};

#[test]
fn the_words_of_a_command_that_changes_a_machine_are_read_in_any_shell() {
  for command in [
    "sudo apt-get install -y ffmpeg",
    "winget install Git.Git",
    "Stop-Process -Name chrome -Force",
    "pkill -f node",
    "systemctl --user restart bmo-web",
    "shutdown /r /t 0",
  ] {
    assert!(changes_machine(command).is_some(), "{command}");
  }

  for command in ["Get-Date", "ls -la ~/work", "cat /etc/os-release", "echo killing time"] {
    assert_eq!(changes_machine(command), None, "{command}");
  }
}

#[tokio::test]
async fn a_command_that_changes_the_machine_runs_only_once_the_owner_chose_to_run_it() {
  let stood = owned().await;

  *stood.world.frames.lock().unwrap() = vec![
    frame("started", json!({ "id": "c1" })),
    frame("exit", json!({ "code": 0, "signal": null, "canceled": false })),
  ];

  let command = "winget install Git.Git";
  let first = answer(
    &handed("pc-agent", &[], json!({ "node": "pc-four", "command": command })),
    &stood.ground,
  )
  .await;

  assert!(first.is_error && first.text().contains("question #1"), "{}", first.text());
  assert!(stood.world.execs.lock().unwrap().is_empty(), "nothing runs before the approval");
  {
    let questions = stood.world.questions.lock().unwrap();

    assert_eq!(questions[0].0["text"], asked_text("pc-four", command));
    assert_eq!(questions[0].0["choices"], json!([RUN, SKIP]));
    assert_eq!(questions[0].0["askerName"], "pc-agent");
  }

  let again = |approval: i64| {
    let arguments = json!({ "node": "pc-four", "command": command, "approval": approval });
    let ground = &stood.ground;

    async move { answer(&handed("pc-agent", &[], arguments), ground).await }
  };

  // Still open, or settled by a supervisor or by its recommendation, it approves nothing.
  assert!(again(1).await.is_error);

  for (source, choice) in [("supervisor", RUN), ("recommendation", SKIP), ("person", SKIP)] {
    {
      let mut questions = stood.world.questions.lock().unwrap();

      questions[0].0["state"] = json!("answered");
      questions[0].1 = vec![json!({ "settles": true, "source": source, "choice": choice })];
    }

    assert!(again(1).await.is_error, "{source} {choice}");
  }

  // The owner's own choice to run it runs it, and another command never rides on it.
  stood.world.questions.lock().unwrap()[0].1 =
    vec![json!({ "settles": true, "source": "person", "choice": RUN })];

  let other = answer(
    &handed(
      "pc-agent",
      &[],
      json!({ "node": "pc-four", "command": "winget install Other", "approval": 1 }),
    ),
    &stood.ground,
  )
  .await;

  assert!(other.is_error, "{}", other.text());

  let ran = again(1).await;

  assert!(!ran.is_error, "{}", ran.text());
  assert_eq!(stood.world.execs.lock().unwrap().len(), 1);
}
