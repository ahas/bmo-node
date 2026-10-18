//! An ssh node's check: one batch login that runs nothing, started and read as the shared policy
//! says (docs/nodes.md, 「How a node's state is known」).

use std::process::Stdio;
use std::time::Duration;

use policy::ssh::{Login, SSH};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::rows::SshTarget;
use crate::{Machine, Met, NodeState, last_line};

/// How a login ended: its exit code, none where its wait ran out, and what it said on its standard
/// error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshRun {
  pub code: Option<i32>,
  pub stderr: String,
}

/// The arguments a kind of login is started with, `command` run there where it is given.
pub fn login_args(
  machine: &Machine,
  login: &Login,
  target: &SshTarget,
  command: Option<&str>,
) -> Vec<String> {
  let mut args = Vec::new();
  let mut option = |value: &str| args.extend(["-o".to_owned(), value.to_owned()]);

  login.options.iter().for_each(|one| option(one));

  if let Some(key) = &target.key_path {
    login.key_options.iter().for_each(|one| option(one));
    args.extend(["-i".to_owned(), machine.expand_home(key).to_string_lossy().into_owned()]);
  }

  args.extend(["-p".to_owned(), target.port.to_string(), format!("{}@{}", target.user, target.host)]);
  args.extend(command.map(str::to_owned));

  args
}

/// What a login that ended met, read off its exit and its last words.
pub fn read_run(run: &SshRun) -> Met {
  let Some(code) = run.code else {
    return Met::new(NodeState::Unreachable, "the login did not finish in time");
  };

  if code == 0 {
    return Met::connected();
  }

  let detail = last_line(&run.stderr);

  match SSH.ended_state(&run.stderr) {
    Some(state) => Met { state, detail },
    None => Met::new(NodeState::Error, detail.unwrap_or_else(|| format!("ssh ended with {code}"))),
  }
}

/// Runs OpenSSH with `args`, stopped once `wait` has passed.
pub async fn run(machine: &Machine, args: &[String], wait: Duration) -> SshRun {
  let started = Command::new(&machine.ssh)
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn();
  let mut child = match started {
    Ok(child) => child,
    Err(error) => {
      return SshRun {
        code: Some(-1),
        stderr: format!("ssh could not be started: {error}"),
      };
    }
  };
  let mut stderr = child.stderr.take();
  let ended = tokio::time::timeout(wait, async {
    let mut said = String::new();

    if let Some(stream) = stderr.as_mut() {
      let _ = stream.read_to_string(&mut said).await;
    }

    (child.wait().await, said)
  })
  .await;

  match ended {
    Ok((status, said)) => SshRun {
      code: Some(status.ok().and_then(|status| status.code()).unwrap_or(-1)),
      stderr: said,
    },
    Err(_) => SshRun {
      code: None,
      stderr: String::new(),
    },
  }
}

/// One ssh node checked: a key the row names that is not there is said without trying the login.
pub async fn check(machine: &Machine, target: &SshTarget) -> Met {
  if let Some(key) = &target.key_path
    && !machine.expand_home(key).exists()
  {
    return Met::new(NodeState::Error, format!("no key at {key}"));
  }

  let args = login_args(machine, &SSH.batch, target, Some(&SSH.check.command));

  read_run(&run(machine, &args, Duration::from_secs(SSH.check.wait_seconds)).await)
}
