//! The nodes and the node checks (docs/api.md, 「Nodes」 and 「Node checks」): the list and one node, its
//! target, the registry's changes and the check, each publishing the node's own events.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{StatusCode, Uri};
use bmo_node_client::nodes::{
  NewNode, Node, NodeChanges, NodeCheck, NodeCheckAsk, NodePage, NodeRemoveQuery, NodesQuery, Target,
};
use node_check::pass::check_nodes;
use store::nodes::{DEFAULT_LOGIN_SHELL, DEFAULT_SSH_PORT, NodeChanges as StoreChanges, SshLogin};

use super::{Shared, node, settle};
use crate::Service;
use crate::reply::{Answer, Done, Made, Reply, body, query};
use crate::views;
use bmo_node_client::ErrorCode;

/// Every node as the routes answer it, in the list's order.
fn all(service: &Service) -> Result<Vec<Node>, Reply> {
  let now = store::now_seconds();

  Ok(service.store.nodes()?.iter().map(|node| service.node_view(node, now)).collect())
}

fn one_view(service: &Service, name: &str) -> Result<Node, Reply> {
  Ok(service.node_view(&node(service, name)?, store::now_seconds()))
}

/// The page of nodes after the one `after` names, at most `limit` of them.
fn page(nodes: Vec<Node>, asked: NodesQuery) -> Result<NodePage, Reply> {
  let start = match &asked.after {
    None => 0,
    Some(after) => {
      nodes
        .iter()
        .position(|node| node.name == *after)
        .ok_or_else(|| Reply::bad_request(format!("after names no listed node: {after}")))?
        + 1
    }
  };
  let limit = asked.limit.map_or(usize::MAX, |limit| limit as usize);
  let rest = nodes.len().saturating_sub(start);
  let items: Vec<Node> = nodes.into_iter().skip(start).take(limit).collect();

  Ok(NodePage {
    more: rest > items.len(),
    items,
  })
}

pub async fn list(service: Shared, uri: Uri) -> Answer<NodePage> {
  let asked: NodesQuery = query(&uri)?;

  Ok(Json(page(all(&service)?, asked)?))
}

pub async fn one(service: Shared, Path(name): Path<String>) -> Answer<Node> {
  Ok(Json(one_view(&service, &name)?))
}

pub async fn target(service: Shared, Path(name): Path<String>) -> Answer<Target> {
  Ok(Json(views::target(&node(&service, &name)?, &service.machine)?))
}

pub async fn create(service: Shared, bytes: Bytes) -> Made<Node> {
  let asked: NewNode = body(&bytes)?;
  let login = SshLogin {
    host: asked.host,
    port: asked.port.unwrap_or(DEFAULT_SSH_PORT),
    user: asked.user,
    key_path: asked.key_path,
    shell: asked
      .shell
      .map_or_else(|| DEFAULT_LOGIN_SHELL.to_owned(), |shell| shell.name().to_owned()),
  };
  let node = service.store.add_node(&asked.name, asked.label.as_deref(), asked.role, login)?;

  settle(&service)?;
  Ok((StatusCode::CREATED, Json(service.node_view(&node, store::now_seconds()))))
}

pub async fn change(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer<Node> {
  let asked: NodeChanges = body(&bytes)?;
  let changes = StoreChanges {
    label: asked.label,
    role: asked.role,
    ownership: asked.ownership,
    account: asked.account,
    host: asked.host,
    user: asked.user,
    port: asked.port,
    key_path: asked.key_path,
    shell: asked.shell.map(|shell| shell.name().to_owned()),
  };
  let node = service.store.update_node(&name, changes)?;

  settle(&service)?;
  Ok(Json(service.node_view(&node, store::now_seconds())))
}

pub async fn remove(service: Shared, Path(name): Path<String>, uri: Uri) -> Done {
  let asked: NodeRemoveQuery = query(&uri)?;

  // A hub node whose agent is still attached would come back on the next pass, so its agent is sent a leave
  // through its hub first, and nothing is removed where the hub does not take it (docs/nodes.md).
  if asked.leave == Some(true) {
    let Some(node) = service.store.node(&name)? else {
      return Err(Reply::no_node(&name));
    };

    if let (Some(agent), Some(url)) = (node.agent.as_deref(), node.hub_url.as_deref()) {
      let hub = service
        .store
        .hubs()?
        .into_iter()
        .find(|hub| hub.url == url)
        .ok_or_else(|| Reply::new(ErrorCode::Conflict, format!("no listed hub is at {url}")))?;
      let target = node_check::rows::Hub {
        url: hub.url,
        token_file: hub.token_file,
      };

      node_check::hub::leave(&service.machine, &target, agent)
        .await
        .map_err(|why| Reply::new(ErrorCode::Conflict, why))?;
    }
  }

  if !service.store.remove_node(&name)? {
    return Err(Reply::no_node(&name));
  }

  settle(&service)?;
  Ok(StatusCode::NO_CONTENT)
}

/// The nodes named, or every node, checked now and written to their rows; then each as it stands.
pub async fn check(service: Shared, bytes: Bytes) -> Made<NodeCheck> {
  let asked: NodeCheckAsk = body(&bytes)?;

  for name in &asked.node_names {
    if !service.store.node_exists(name)? {
      return Err(Reply::bad_field(format!("nodeNames names no listed node: {name}")));
    }
  }

  if asked.node_names.is_empty() {
    check_nodes(&*service.store, &service.machine, None).await?;
  } else {
    for name in &asked.node_names {
      check_nodes(&*service.store, &service.machine, Some(name)).await?;
    }
  }

  settle(&service)?;

  let items = match asked.node_names.as_slice() {
    [] => all(&service)?,
    names => names.iter().filter_map(|name| one_view(&service, name).ok()).collect(),
  };

  Ok((StatusCode::CREATED, Json(NodeCheck { items })))
}
