//! A node as every route answers it (docs/api.md, 「Nodes」): the node, its connection and its batch
//! login. None of them holds a credential, only where one is kept.

use bmo_node_client::nodes::{JakeTarget, LoginShell, Node as NodeView, SshTarget as SshConnection, Target};
use bmo_node_client::{ErrorCode, nodes};
use node_check::Machine;
use node_check::rows::SshTarget;
use node_check::ssh::{login_args, login_options};
use policy::node_states::{NodeState, StateTone};
use policy::nodes::NodeKind;
use policy::ssh::SSH;
use store::nodes::{JOBS_SERVICE, Node};

use crate::reply::Reply;
use crate::this_machine::ThisMachine;

/// How long after its last check a connected node's link still stands: two passes and the check's own
/// wait (docs/nodes.md, 「A node's link」).
pub const LINK_WINDOW_SECONDS: i64 = 180;

/// Milliseconds in a second: the store keeps its moments in seconds, and every answer gives them in
/// epoch milliseconds.
const MS_PER_SECOND: i64 = 1_000;

/// Whether an ssh node is this machine; a jake node never is, since its hub says nothing of where its
/// agent runs.
pub fn is_this_machine(node: &Node, here: &ThisMachine) -> bool {
  node.kind == NodeKind::Ssh && node.host.as_deref().is_some_and(|host| here.names(host))
}

/// Whether a node's last check found it connected within the link window, at `now` in seconds.
pub fn is_up(node: &Node, now: i64) -> bool {
  node.state == NodeState::Connected && node.checked_at.is_some_and(|at| now - at <= LINK_WINDOW_SECONDS)
}

/// A node as every route answers it, at `now` in seconds.
pub fn view(node: &Node, here: &ThisMachine, now: i64) -> NodeView {
  let this_machine = is_this_machine(node, here);
  let takes_jobs = node.kind == NodeKind::Ssh
    && !this_machine
    && !node.powershell()
    && (node.role.is_none() || node.services().contains(&JOBS_SERVICE));

  NodeView {
    name: node.name.clone(),
    label: node.label.clone(),
    kind: node.kind,
    agent: node.agent.clone(),
    address: node.address(),
    credential: node.credential().map(str::to_owned),
    role: node.role.clone(),
    state: node.state,
    tone: node.state.tone(),
    detail: node.detail.clone(),
    checked_at: node.checked_at.map(|at| at * MS_PER_SECOND),
    connected_at: node.connected_at.map(|at| at * MS_PER_SECOND),
    up: is_up(node, now),
    this_machine,
    takes_jobs,
  }
}

fn incomplete(node: &Node) -> Reply {
  Reply::new(ErrorCode::Incomplete, format!("{} is missing a field its connection type needs", node.name))
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

/// The connection a node is reached by, or why the row lacks one.
pub fn target(node: &Node) -> Result<Target, Reply> {
  match node.kind {
    NodeKind::Jake => {
      let (Some(hub_url), Some(token_file), Some(agent)) = (&node.hub_url, &node.token_file, &node.agent)
      else {
        return Err(incomplete(node));
      };

      Ok(Target::Jake(JakeTarget {
        hub_url: hub_url.clone(),
        token_file: token_file.clone(),
        agent: agent.clone(),
        role: node.role.clone(),
      }))
    }
    NodeKind::Ssh => {
      let login = ssh_target(node)?;

      Ok(Target::Ssh(SshConnection {
        options: login_options(&SSH.batch, &login),
        host: login.host,
        port: login.port,
        user: login.user,
        key_path: login.key_path,
        shell: LoginShell::of_code(&node.shell).unwrap_or_default(),
        role: node.role.clone(),
      }))
    }
  }
}

/// The arguments that open a batch login to an ssh node, or why it has none.
pub fn login(node: &Node, machine: &Machine) -> Result<nodes::Login, Reply> {
  if node.kind == NodeKind::Jake {
    return Err(Reply::new(
      ErrorCode::NotSsh,
      format!("{} is reached through a hub and has no SSH route", node.name),
    ));
  }

  Ok(nodes::Login {
    args: login_args(machine, &SSH.batch, &ssh_target(node)?, None),
  })
}
