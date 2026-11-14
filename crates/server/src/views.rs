//! A node as every route answers it (docs/api.md, 「Nodes」): the node and its target. Neither holds a
//! credential, only where one is kept.

use bmo_node_client::ErrorCode;
use bmo_node_client::nodes::{LoginShell, Node as NodeView, Target};
use node_check::Machine;
use node_check::rows::SshTarget;
use node_check::ssh::{login_args, login_options};
use policy::node_states::{NodeState, StateTone};
use policy::nodes::NodeKind;
use policy::ssh::SSH;
use store::nodes::Node;

use crate::reply::Reply;
use crate::this_machine::ThisMachine;

/// How long after its last check a connected node's link still stands: two passes and the check's own
/// wait (docs/nodes.md, 「A node's link」).
pub const LINK_WINDOW_SECONDS: i64 = 180;

/// Milliseconds in a second: the store keeps its moments in seconds, and every answer gives them in
/// epoch milliseconds.
pub const MS_PER_SECOND: i64 = 1_000;

/// Whether an ssh node is this machine; a hub node never is, since its hub says nothing of where its agent
/// runs.
pub fn is_this_machine(node: &Node, here: &ThisMachine) -> bool {
  node.kind == NodeKind::Ssh && node.host.as_deref().is_some_and(|host| here.names(host))
}

/// Whether a node's last check found it connected within the link window, at `now` in seconds.
pub fn is_up(node: &Node, now: i64) -> bool {
  node.state == NodeState::Connected && node.checked_at.is_some_and(|at| now - at <= LINK_WINDOW_SECONDS)
}

/// A node as every route answers it, at `now` in seconds.
pub fn view(node: &Node, here: &ThisMachine, now: i64) -> NodeView {
  NodeView {
    name: node.name.clone(),
    label: node.label.clone(),
    kind: node.kind,
    hub_id: node.hub_id,
    agent_name: node.agent.clone(),
    address: node.address(),
    credential: node.credential().map(str::to_owned),
    role: node.role.clone(),
    callers: node.callers().into_iter().map(str::to_owned).collect(),
    state: node.state,
    tone: node.state.tone(),
    detail: node.detail.clone(),
    checked_at: node.checked_at.map(|at| at * MS_PER_SECOND),
    connected_at: node.connected_at.map(|at| at * MS_PER_SECOND),
    up: is_up(node, now),
    this_machine: is_this_machine(node, here),
  }
}

/// Whether a node moved between two views, its check's times aside: a check that moved nothing else
/// publishes nothing.
pub fn moved(before: &NodeView, after: &NodeView) -> bool {
  let untimed = |view: &NodeView| NodeView {
    checked_at: None,
    connected_at: None,
    ..view.clone()
  };

  untimed(before) != untimed(after)
}

fn incomplete(node: &Node) -> Reply {
  Reply::new(
    ErrorCode::Incomplete,
    format!("{} is missing a field its connection type needs", node.name),
  )
}

/// An ssh node's login target, or why the row has none.
fn ssh_target(node: &Node) -> Result<SshTarget, Reply> {
  let (Some(host), Some(user)) = (&node.host, &node.user) else {
    return Err(incomplete(node));
  };

  Ok(SshTarget {
    host: host.clone(),
    port: u16::try_from(node.ssh_port()).map_err(|_| Reply::failed("the node's port is out of range"))?,
    user: user.clone(),
    key_path: node.key_path.clone(),
  })
}

/// How a node is reached, with its batch login laid out as `ssh` arguments by `machine`'s home, or why the
/// row lacks a field its connection type needs.
pub fn target(node: &Node, machine: &Machine) -> Result<Target, Reply> {
  let mut target = Target {
    node_name: node.name.clone(),
    kind: node.kind,
    role: node.role.clone(),
    host: None,
    port: None,
    user: None,
    key_path: None,
    shell: None,
    ssh_options: Vec::new(),
    ssh_args: Vec::new(),
    hub_id: None,
    hub_address: None,
    hub_token_file: None,
    agent_name: None,
  };

  match node.kind {
    NodeKind::Hub => {
      let (Some(hub_id), Some(hub_url), Some(token_file), Some(agent)) =
        (node.hub_id, &node.hub_url, &node.token_file, &node.agent)
      else {
        return Err(incomplete(node));
      };

      target.hub_id = Some(hub_id);
      target.hub_address = Some(hub_url.clone());
      target.hub_token_file = Some(token_file.clone());
      target.agent_name = Some(agent.clone());
    }
    NodeKind::Ssh => {
      let login = ssh_target(node)?;

      target.ssh_options = login_options(&SSH.batch, &login);
      target.ssh_args = login_args(machine, &SSH.batch, &login, None);
      target.host = Some(login.host);
      target.port = Some(login.port);
      target.user = Some(login.user);
      target.key_path = login.key_path;
      target.shell = Some(LoginShell::of_code(&node.shell).unwrap_or_default());
    }
  }

  Ok(target)
}
