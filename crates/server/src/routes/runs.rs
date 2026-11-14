//! A run (docs/api.md, 「Runs」): one command a caller runs on a hub node through its hub, for a person whose
//! own node it is, judged before the hub is asked anything and answered with the command's end.

use axum::body::Bytes;
use axum::extract::Path;
use axum::{Extension, Json};
use bmo_node_client::ErrorCode;
use bmo_node_client::nodes::LoginShell;
use bmo_node_client::runs::{Run, RunAsk, RunShell};
use node_check::hub_exec::{Broke, ExecBody, run as run_on_hub};
use node_check::rows::Hub;
use policy::nodes::NodeKind;
use store::nodes::Node;

use super::{Shared, node};
use crate::Service;
use crate::reply::{Answer, Reply, body};

/// The caller a run is made as, named by its own token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCaller(pub String);

fn forbidden(error: impl Into<String>) -> Reply {
  Reply::new(ErrorCode::Forbidden, error)
}

fn upstream(error: impl Into<String>) -> Reply {
  Reply::new(ErrorCode::UpstreamFailed, error)
}

/// Whether `node` lets the caller onto it, or the refusal naming both.
fn let_in(node: &Node, caller: &str) -> Result<(), Reply> {
  if node.lets(caller) {
    return Ok(());
  }

  let callers = match node.callers.as_deref() {
    Some(callers) => format!("its callers are {callers}"),
    None => "it lets no caller".to_owned(),
  };

  Err(forbidden(format!(
    "The node {} does not let the caller {caller} make a run: {callers}.",
    node.name
  )))
}

/// Whether the node is the person's own whose account is `account`, or the refusal saying whose it is.
async fn owned(service: &Service, node: &str, caller: &str, account: &str) -> Result<(), Reply> {
  let Some(read) = &service.owners else {
    return Err(forbidden(format!(
      "No owners' read is configured, so the node {node} is no one's and no run is let through."
    )));
  };
  let owners = read
    .owners(service.machine.http(), node, caller)
    .await
    .map_err(|why| upstream(format!("Whose node {node} is could not be read: {why}.")))?;

  if owners.iter().any(|owner| owner == account) {
    return Ok(());
  }

  Err(forbidden(match owners.as_slice() {
    [] => format!("The node {node} is no person's node, so no run is made on it for account {account}."),
    owners => {
      format!("The node {node} is account {}'s, not account {account}'s.", owners.join(", account "))
    }
  }))
}

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
  Extension(RunCaller(caller)): Extension<RunCaller>,
  bytes: Bytes,
) -> Answer<Run> {
  let asked: RunAsk = body(&bytes)?;
  let node = node(&service, &name)?;

  let_in(&node, &caller)?;

  if node.kind != NodeKind::Hub {
    return Err(Reply::new(
      ErrorCode::NotAHubNode,
      format!(
        "The node {name} is an ssh node, which a run does not reach: log in with its target's sshArgs."
      ),
    ));
  }

  let (Some(url), Some(token_file), Some(agent)) = (&node.hub_url, &node.token_file, &node.agent) else {
    return Err(Reply::new(
      ErrorCode::Incomplete,
      format!("{name} is missing a field its connection type needs"),
    ));
  };

  owned(&service, &name, &caller, &asked.account).await?;

  let shell = asked.shell.unwrap_or_else(|| RunShell::of_host(LoginShell::of_code(&node.shell)));
  let exec = ExecBody {
    shell: shell.name().to_owned(),
    command: asked.command,
    cwd: asked.cwd,
    env: asked.env,
    distro: asked.distro,
  };
  let hub = Hub {
    url: url.clone(),
    token_file: token_file.clone(),
  };

  run_on_hub(&service.machine, &hub, agent, &name, &exec)
    .await
    .map(Json)
    .map_err(|why| broke(&name, &why))
}
