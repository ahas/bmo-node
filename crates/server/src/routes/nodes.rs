//! The nodes and the hubs (docs/api.md, 「Nodes」 and 「Hubs」): the list and one node, the check, a node's
//! connection and login, and the registry's changes, each publishing `nodes-changed`.

use axum::Json;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{StatusCode, Uri};
use bmo_node_client::ErrorCode;
use bmo_node_client::nodes::{
  CheckNodes, Hub, HubQuery, HubRemoved, Login, NewNode, Node, NodeChanges, NodeList, NodeListQuery,
  Removed, Target,
};
use node_check::pass::check_nodes;
use store::nodes::{DEFAULT_LOGIN_SHELL, DEFAULT_SSH_PORT, NodeChanges as StoreChanges, SshLogin};

use super::{Shared, node, now_seconds};
use crate::Service;
use crate::reply::{Answer, Made, Reply, body, query};
use crate::views;

/// Every node as the routes answer it, in the list's order.
fn all(service: &Service) -> Result<Vec<Node>, Reply> {
  let now = now_seconds();

  Ok(service.store.nodes()?.iter().map(|node| views::view(node, &service.here, now)).collect())
}

/// The nodes after the one `after` names, at most `limit` of them.
fn page(nodes: Vec<Node>, asked: NodeListQuery) -> Result<Vec<Node>, Reply> {
  let start = match &asked.after {
    None => 0,
    Some(after) => nodes.iter().position(|node| node.name == *after).ok_or_else(|| Reply::no_node(after))? + 1,
  };
  let limit = asked.limit.map_or(usize::MAX, |limit| limit as usize);

  Ok(nodes.into_iter().skip(start).take(limit).collect())
}

pub async fn list(service: Shared, uri: Uri) -> Answer<NodeList> {
  let asked: NodeListQuery = query(&uri)?;

  Ok(Json(NodeList {
    nodes: page(all(&service)?, asked)?,
  }))
}

pub async fn one(service: Shared, Path(name): Path<String>) -> Answer<Node> {
  Ok(Json(views::view(&node(&service, &name)?, &service.here, now_seconds())))
}

pub async fn target(service: Shared, Path(name): Path<String>) -> Answer<Target> {
  Ok(Json(views::target(&node(&service, &name)?)?))
}

pub async fn login(service: Shared, Path(name): Path<String>) -> Answer<Login> {
  Ok(Json(views::login(&node(&service, &name)?, &service.machine)?))
}

/// The one node named, or every node, checked now and written to its row; then the whole list.
pub async fn check(service: Shared, bytes: Bytes) -> Answer<NodeList> {
  let asked: CheckNodes = body(&bytes)?;

  if let Some(name) = &asked.name {
    node(&service, name)?;
  }

  let checked = check_nodes(&*service.store, &service.machine, asked.name.as_deref()).await?;

  if checked.changed {
    service.events.nodes_changed();
  }

  Ok(Json(NodeList { nodes: all(&service)? }))
}

pub async fn add(service: Shared, bytes: Bytes) -> Made<Node> {
  let asked: NewNode = body(&bytes)?;
  let login = SshLogin {
    host: asked.host,
    port: asked.port.unwrap_or(DEFAULT_SSH_PORT),
    user: asked.user,
    key_path: asked.key_path,
    shell: asked.shell.map_or_else(|| DEFAULT_LOGIN_SHELL.to_owned(), |shell| shell.name().to_owned()),
  };
  let node = service.store.add_node(&asked.name, asked.label.as_deref(), asked.role, login)?;

  service.events.nodes_changed();
  Ok((StatusCode::CREATED, Json(views::view(&node, &service.here, now_seconds()))))
}

pub async fn set(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer<Node> {
  let asked: NodeChanges = body(&bytes)?;
  let changes = StoreChanges {
    label: asked.label,
    role: asked.role,
    host: asked.host,
    user: asked.user,
    port: asked.port,
    key_path: asked.key_path,
    shell: asked.shell.map(|shell| shell.name().to_owned()),
  };
  let node = service.store.update_node(&name, changes)?;

  service.events.nodes_changed();
  Ok(Json(views::view(&node, &service.here, now_seconds())))
}

pub async fn remove(service: Shared, Path(name): Path<String>) -> Answer<Removed> {
  if !service.store.remove_node(&name)? {
    return Err(Reply::no_node(&name));
  }

  service.events.nodes_changed();
  Ok(Json(Removed { removed: name }))
}

pub async fn add_hub(service: Shared, bytes: Bytes) -> Made<Hub> {
  let asked: Hub = body(&bytes)?;
  let hub = service.store.add_hub(&asked.url, &asked.token_file)?;

  service.events.nodes_changed();
  Ok((
    StatusCode::CREATED,
    Json(Hub {
      url: hub.url,
      token_file: hub.token_file,
    }),
  ))
}

pub async fn remove_hub(service: Shared, uri: Uri) -> Answer<HubRemoved> {
  let asked: HubQuery = query(&uri)?;
  let Some(removed) = service.store.remove_hub(&asked.url)? else {
    return Err(Reply::new(ErrorCode::NoHub, format!("no hub is listed at {}", asked.url)));
  };

  service.events.nodes_changed();
  Ok(Json(HubRemoved {
    url: store::nodes::checked_hub_url(&asked.url)?,
    removed,
  }))
}
