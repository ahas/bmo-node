//! A node as each reader reads it (docs/api.md): agency's row, job's row and reach, the console's view,
//! and the connection a tool reaches it by. None of them holds a credential, only where one is kept.

use policy::nodes::NodeKind;
use serde_json::{Value, json};
use store::nodes::{JOBS_SERVICE, Node};

use crate::reply::Reply;
use crate::this_machine::ThisMachine;

/// A row as agency's node list reads it.
pub fn agency_row(node: &Node) -> Value {
  json!({
    "node": node.name,
    "label": node.label,
    "kind": node.kind.name(),
    "state": node.state.name(),
    "detail": node.detail,
  })
}

/// A row as job's node list reads it.
pub fn job_row(node: &Node) -> Value {
  json!({
    "name": node.name,
    "label": node.label,
    "kind": node.kind.name(),
    "role": node.role,
    "state": node.state.name(),
    "checkedAt": node.checked_at,
  })
}

/// An ssh node's reach, or why it has none.
pub fn reach(node: &Node) -> Result<Value, Reply> {
  if node.kind == NodeKind::Jake {
    return Err(Reply::refused(
      "not-ssh",
      format!("{} is reached through a hub and has no SSH route", node.name),
    ));
  }

  let (Some(host), Some(user)) = (&node.host, &node.user) else {
    return Err(incomplete(node));
  };

  Ok(json!({
    "host": host,
    "port": node.ssh_port(),
    "user": user,
    "keyPath": node.key_path,
    "shell": node.shell,
  }))
}

fn incomplete(node: &Node) -> Reply {
  Reply::refused("incomplete", format!("{} is missing a field its connection type needs", node.name))
}

/// Whether an ssh node is this machine; a jake node never is, since its hub says nothing of where its
/// agent runs.
pub fn is_this_machine(node: &Node, here: &ThisMachine) -> bool {
  node.kind == NodeKind::Ssh && node.host.as_deref().is_some_and(|host| here.names(host))
}

/// The console's view of a node.
pub fn view(node: &Node, here: &ThisMachine) -> Value {
  let this_machine = is_this_machine(node, here);
  let takes_jobs = node.kind == NodeKind::Ssh
    && !this_machine
    && !node.powershell()
    && (node.role.is_none() || node.services().contains(&JOBS_SERVICE));

  json!({
    "name": node.name,
    "label": node.label,
    "kind": node.kind.name(),
    "agent": node.agent,
    "address": node.address(),
    "credential": node.credential(),
    "role": node.role,
    "state": node.state.name(),
    "tone": node.state.tone().name(),
    "detail": node.detail,
    "checkedAt": node.checked_at,
    "connectedAt": node.connected_at,
    "thisMachine": this_machine,
    "takesJobs": takes_jobs,
  })
}

/// The connection a tool outside the service reaches a node by, or why the row lacks one.
pub fn target(node: &Node) -> Result<Value, Reply> {
  match node.kind {
    NodeKind::Jake => {
      let (Some(hub_url), Some(token_file), Some(agent)) = (&node.hub_url, &node.token_file, &node.agent)
      else {
        return Err(incomplete(node));
      };

      Ok(
        json!({ "kind": "jake", "hubUrl": hub_url, "tokenFile": token_file, "agent": agent, "role": node.role }),
      )
    }
    NodeKind::Ssh => {
      let (Some(host), Some(user)) = (&node.host, &node.user) else {
        return Err(incomplete(node));
      };

      Ok(json!({
        "kind": "ssh",
        "host": host,
        "port": node.ssh_port(),
        "user": user,
        "keyPath": node.key_path,
        "shell": node.shell,
        "role": node.role,
      }))
    }
  }
}
