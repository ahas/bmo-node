//! A node checked, every node checked, and each outcome written to its row through the store the caller
//! holds.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use futures::future::join_all;

use crate::hub::{self, HubAnswer};
use crate::rows::{Hub, NodeRow, Target};
use crate::{Asking, Machine, Met, NodeCheck, NodeState, now_seconds, ssh};

/// One node checked, by its own login or by asking its own hub.
pub async fn check_node(machine: &Machine, node: &NodeRow) -> NodeCheck {
  let met = match node.target() {
    None => Met::new(NodeState::Error, "the row is missing a field its connection type needs"),
    Some(Target::Ssh(target)) => ssh::check(machine, &target).await,
    Some(Target::Jake { hub, agent }) => hub::ask(machine, &hub).await.state_of(&agent),
  };

  met.at(now_seconds())
}

/// Every hub asked once, together, each answer beside its hub's address.
pub async fn ask_hubs(machine: &Machine, hubs: &[Hub]) -> Vec<(String, HubAnswer)> {
  join_all(hubs.iter().map(async |hub| (hub.url.clone(), hub::ask(machine, hub).await))).await
}

/// Every row checked together: a jake node whose hub was asked already read off that answer, every
/// other node checked by itself.
pub async fn check_rows(
  machine: &Machine,
  nodes: &[NodeRow],
  answers: &HashMap<String, HubAnswer>,
) -> Vec<NodeCheck> {
  join_all(nodes.iter().map(async |node| {
    let answered = node.jake().and_then(|(hub, agent)| Some((answers.get(hub)?, agent)));

    match answered {
      Some((answer, agent)) => answer.state_of(agent).at(now_seconds()),
      None => check_node(machine, node).await,
    }
  }))
  .await
}

/// The node list as a store holds it, which a pass reads and writes.
pub trait NodeRows {
  type Error;

  fn hubs(&self) -> impl Future<Output = Result<Vec<Hub>, Self::Error>> + Send;
  /// Every node, in the list's order.
  fn list(&self) -> impl Future<Output = Result<Vec<NodeRow>, Self::Error>> + Send;
  /// The jake node for an agent attached to a hub, added where none stands for it yet, its shell the one the hub
  /// says its host takes.
  fn add_agent(
    &self,
    hub: &str,
    agent: &str,
    shell: &str,
  ) -> impl Future<Output = Result<(), Self::Error>> + Send;
  /// The jake node for an agent its hub says has departed, removed; whether there was one.
  fn remove_agent(&self, hub: &str, agent: &str) -> impl Future<Output = Result<bool, Self::Error>> + Send;
  fn exists(&self, name: &str) -> impl Future<Output = Result<bool, Self::Error>> + Send;
  fn record(&self, name: &str, check: &NodeCheck) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// What a pass came to: whether the list or any node's state moved, and how many nodes it checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checked {
  pub changed: bool,
  pub checked: usize,
}

/// Every node checked at once, or the one named, and each outcome written to its row as it lands. A whole
/// pass asks every hub once, adds a node for each agent attached to a listed hub that none stands for yet,
/// removes the node of each agent a hub says has departed, and reads every jake node's state off its
/// hub's answer. It leaves to a check still out from another pass the hub or the node that check asks, and
/// a jake node whose hub it so left.
pub async fn check_nodes<S: NodeRows>(
  store: &S,
  machine: &Machine,
  only: Option<&str>,
) -> Result<Checked, S::Error> {
  if let Some(name) = only {
    let nodes: Vec<NodeRow> = store.list().await?.into_iter().filter(|node| node.name == name).collect();
    let checks = check_rows(machine, &nodes, &HashMap::new()).await;
    let mut changed = false;

    for (node, check) in nodes.iter().zip(&checks) {
      changed |= record(store, node, check).await?;
    }

    return Ok(Checked {
      changed,
      checked: checks.len(),
    });
  }

  let mut hubs = Vec::new();
  let mut left = HashSet::new();

  for hub in store.hubs().await? {
    match machine.out.take(format!("hub {}", hub.url)) {
      Some(asking) => hubs.push((hub, asking)),
      None => {
        left.insert(hub.url);
      }
    }
  }

  let asked =
    join_all(hubs.iter().map(async |(hub, _)| (hub.url.clone(), hub::ask(machine, hub).await))).await;

  drop(hubs);

  let before = store.list().await?.len();
  let mut removed = false;

  // In the hubs' order and each hub's own, which is the order the new nodes stand in.
  for (hub, answer) in &asked {
    for agent in answer.agents() {
      store.add_agent(hub, agent, answer.shell_of(agent)).await?;
    }

    // An uninstalled agent's machine is gone, not down, so its node leaves the list.
    for agent in answer.departed() {
      removed |= store.remove_agent(hub, agent).await?;
    }
  }

  let answers: HashMap<String, HubAnswer> = asked.into_iter().collect();
  let nodes = store.list().await?;
  let listed = nodes.len();
  let taken: Vec<(&NodeRow, Asking)> = nodes
    .iter()
    .filter(|node| node.jake().is_none_or(|(hub, _)| !left.contains(hub)))
    .filter_map(|node| Some((node, machine.out.take(format!("node {}", node.name))?)))
    .collect();
  let landed = join_all(taken.iter().map(async |(node, _)| {
    let answered = node.jake().and_then(|(hub, agent)| Some((answers.get(hub)?, agent)));
    let check = match answered {
      Some((answer, agent)) => answer.state_of(agent).at(now_seconds()),
      None => check_node(machine, node).await,
    };

    record(store, node, &check).await
  }))
  .await;
  let mut changed = removed || listed != before;

  for one in landed {
    changed |= one?;
  }

  Ok(Checked {
    changed,
    checked: taken.len(),
  })
}

/// One check written to its node's row, unless the node was removed while the check was out; whether
/// its state or its detail moved.
async fn record<S: NodeRows>(store: &S, node: &NodeRow, check: &NodeCheck) -> Result<bool, S::Error> {
  if !store.exists(&node.name).await? {
    return Ok(false);
  }

  store.record(&node.name, check).await?;

  Ok(check.state != node.state || check.detail != node.detail)
}
