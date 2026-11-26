//! A run (docs/api.md, 「Runs」): one command a caller runs on a hub node through its hub, for a person whose
//! own node it is, judged before the hub is asked anything and answered with the command's end.

use axum::body::Bytes;
use axum::extract::Path;
use axum::{Extension, Json};
use bmo_node_client::nodes::LoginShell;
use bmo_node_client::runs::{Run, RunAsk, RunShell};
use node_check::hub_exec::{Broke, ExecBody, run as run_on_hub};

use super::{Caller, Doing, Reached, Shared, let_through, upstream};
use crate::reply::{Answer, Reply, body};

/// A command whose end is not known, said with what it said before.
fn broke(node: &str, broke: &Broke) -> Reply {
  let mut error = format!("The command on {node} has no known end: {}.", broke.why);

  for (name, said) in [("stdout", &broke.stdout), ("stderr", &broke.stderr)] {
    if !said.is_empty() {
      error.push_str(&format!("\n--- {name} before that\n{said}"));
    }
  }

  upstream(error)
}

pub async fn run(
  service: Shared,
  Path(name): Path<String>,
  Extension(Caller(caller)): Extension<Caller>,
  bytes: Bytes,
) -> Answer<Run> {
  let asked: RunAsk = body(&bytes)?;
  let Reached { node, hub, agent } =
    let_through(&service, &name, &caller, &asked.account, Doing::Run).await?;

  let shell = asked.shell.unwrap_or_else(|| RunShell::of_host(LoginShell::of_code(&node.shell)));
  let exec = ExecBody {
    shell: shell.name().to_owned(),
    command: asked.command,
    cwd: asked.cwd,
    env: asked.env,
    distro: asked.distro,
  };

  run_on_hub(&service.machine, &hub, &agent, &name, &exec)
    .await
    .map(Json)
    .map_err(|why| broke(&name, &why))
}
