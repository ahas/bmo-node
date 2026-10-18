//! Agency's four calls (agency's `docs/api.md`, 「The node service」): this machine's room, a node's link,
//! the node rows and a node's batch login.

use axum::Json;
use axum::body::Bytes;
use node_check::rows::SshTarget;
use node_check::ssh::login_args;
use policy::node_states::NodeState;
use policy::nodes::NodeKind;
use policy::ssh::SSH;
use serde::Deserialize;
use serde_json::json;

use super::{Shared, node, now_ms, now_seconds};
use crate::reply::{Answer, Reply, body};
use crate::views::agency_row;

/// How long after its last check a connected node's link still stands: two passes and the check's own
/// wait (docs/nodes.md, 「A node's link」).
pub const LINK_WINDOW_SECONDS: i64 = 180;

#[derive(Deserialize)]
struct Named {
  node: String,
}

pub async fn machine_room(service: Shared, _body: Bytes) -> Answer {
  let standing = hardware::machine::here(&service.store, &service.proc_root, &service.data_root, now_ms())?;

  Ok(Json(json!({ "held": standing.held })))
}

pub async fn link_state(service: Shared, bytes: Bytes) -> Answer {
  let asked: Named = body(&bytes)?;
  let now = now_seconds();
  let up = service.store.node(&asked.node)?.is_some_and(|node| {
    node.state == NodeState::Connected && node.checked_at.is_some_and(|at| now - at <= LINK_WINDOW_SECONDS)
  });

  Ok(Json(json!({ "up": up })))
}

pub async fn node_rows(service: Shared, _body: Bytes) -> Answer {
  let nodes: Vec<_> = service.store.nodes()?.iter().map(agency_row).collect();

  Ok(Json(json!({ "nodes": nodes })))
}

pub async fn node_login(service: Shared, bytes: Bytes) -> Answer {
  let asked: Named = body(&bytes)?;
  let node = node(&service, &asked.node)?;
  let ssh = match (node.kind, &node.host, &node.user) {
    (NodeKind::Ssh, Some(host), Some(user)) => {
      let target = SshTarget {
        host: host.clone(),
        port: u16::try_from(node.ssh_port())
          .map_err(|_| Reply::failed("the node's port is out of range"))?,
        user: user.clone(),
        key_path: node.key_path.clone(),
      };

      Some(login_args(&service.machine, &SSH.batch, &target, None))
    }
    _ => None,
  };

  Ok(Json(json!({ "ssh": ssh })))
}
