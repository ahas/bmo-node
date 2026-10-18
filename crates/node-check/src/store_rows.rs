//! The node list a pass reads and writes, as node's own store holds it.

use store::Store;
use store::nodes::{Checked, Node};

use crate::NodeCheck;
use crate::pass::NodeRows;
use crate::rows::{Hub, NodeRow};

/// A node as its check reads it.
pub fn check_row(node: &Node) -> NodeRow {
  NodeRow {
    name: node.name.clone(),
    kind: node.kind.name().to_owned(),
    host: node.host.clone(),
    port: node.port.and_then(|port| u16::try_from(port).ok()),
    user: node.user.clone(),
    key_path: node.key_path.clone(),
    agent: node.agent.clone(),
    hub_url: node.hub_url.clone(),
    token_file: node.token_file.clone(),
    state: node.state,
    detail: node.detail.clone(),
  }
}

impl NodeRows for Store {
  type Error = store::StoreError;

  async fn hubs(&self) -> Result<Vec<Hub>, Self::Error> {
    Ok(
      Store::hubs(self)?
        .into_iter()
        .map(|hub| Hub {
          url: hub.url,
          token_file: hub.token_file,
        })
        .collect(),
    )
  }

  async fn list(&self) -> Result<Vec<NodeRow>, Self::Error> {
    Ok(self.nodes()?.iter().map(check_row).collect())
  }

  async fn add_agent(&self, hub: &str, agent: &str) -> Result<(), Self::Error> {
    Store::add_agent(self, hub, agent).map(drop)
  }

  async fn remove_agent(&self, hub: &str, agent: &str) -> Result<bool, Self::Error> {
    Store::remove_agent(self, hub, agent)
  }

  async fn exists(&self, name: &str) -> Result<bool, Self::Error> {
    self.node_exists(name)
  }

  async fn record(&self, name: &str, check: &NodeCheck) -> Result<(), Self::Error> {
    self.record_check(
      name,
      &Checked {
        state: check.state,
        detail: check.detail.clone(),
        at: check.at,
      },
    )
  }
}
