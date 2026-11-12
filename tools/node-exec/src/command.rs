//! One command run on a hub node through its hub (docs/tools.md, 「A call」): the hub's exec aimed at the
//! node's agent, its output kept up to a bound and its end told apart from a road that failed.

use node_check::Machine;
use node_check::hub_exec::{ExecBody, Said, start};
use node_check::rows::Hub;
use serde::Serialize;

/// The most of each stream an answer keeps: its last part.
pub const KEPT_BYTES: usize = 64 * 1024;

/// One stream of a command's output: its last `KEPT_BYTES`, and whether more came.
#[derive(Debug, Default)]
struct Kept {
  bytes: Vec<u8>,
  cut: bool,
}

impl Kept {
  fn push(&mut self, more: &[u8]) {
    self.bytes.extend_from_slice(more);

    if self.bytes.len() > KEPT_BYTES {
      self.bytes.drain(..self.bytes.len() - KEPT_BYTES);
      self.cut = true;
    }
  }

  fn text(&self) -> String {
    String::from_utf8_lossy(&self.bytes).into_owned()
  }
}

/// A command that ran to its end, as the tool answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ran {
  pub node: String,
  pub shell: String,
  pub exit_code: Option<i64>,
  pub signal: Option<String>,
  pub stdout: String,
  pub stderr: String,
  pub stdout_cut: bool,
  pub stderr_cut: bool,
}

/// A command whose end is not known: why, and what it said before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broke {
  pub why: String,
  pub stdout: String,
  pub stderr: String,
}

/// Runs `body` on `agent` of `hub`, the node being `node`, and waits for its end.
pub async fn run(
  machine: &Machine,
  hub: &Hub,
  agent: &str,
  node: &str,
  body: &ExecBody,
) -> Result<Ran, Broke> {
  let mut stdout = Kept::default();
  let mut stderr = Kept::default();
  let broke = |why: String, stdout: &Kept, stderr: &Kept| Broke {
    why,
    stdout: stdout.text(),
    stderr: stderr.text(),
  };
  let mut running = start(machine, hub, agent, body).await.map_err(|why| broke(why, &stdout, &stderr))?;

  loop {
    match running.next().await {
      Ok(Said::Started(_)) => {}
      Ok(Said::Stdout(bytes)) => stdout.push(&bytes),
      Ok(Said::Stderr(bytes)) => stderr.push(&bytes),
      Ok(Said::Exit { code, signal }) => {
        return Ok(Ran {
          node: node.to_owned(),
          shell: body.shell.clone(),
          exit_code: code,
          signal,
          stdout: stdout.text(),
          stderr: stderr.text(),
          stdout_cut: stdout.cut,
          stderr_cut: stderr.cut,
        });
      }
      Err(why) => return Err(broke(why, &stdout, &stderr)),
    }
  }
}
