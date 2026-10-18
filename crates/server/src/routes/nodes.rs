//! The node list's routes (docs/api.md, 「Node reads」, 「The node list」 and 「Registry routes」): job's
//! rows and reach, the console's views and its check, and the registry's changes, each publishing
//! `nodes-changed`.

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query};
use node_check::pass::check_nodes;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use store::nodes::{DEFAULT_LOGIN_SHELL, DEFAULT_SSH_PORT, NodeChanges, SshLogin};

use super::{Shared, node};
use crate::Service;
use crate::events::NODES_CHANGED;
use crate::reply::{Answer, Reply, body};
use crate::views::{self, job_row};

fn changed(service: &Service) {
  service.events.publish(NODES_CHANGED, json!({}));
}

/// Every node as the console views it, in the list's order.
fn all_views(service: &Service) -> Result<Vec<Value>, Reply> {
  Ok(service.store.nodes()?.iter().map(|node| views::view(node, &service.here)).collect())
}

pub async fn list(service: Shared) -> Answer {
  let nodes: Vec<_> = service.store.nodes()?.iter().map(job_row).collect();

  Ok(Json(json!({ "nodes": nodes })))
}

pub async fn reach(service: Shared, Path(name): Path<String>) -> Answer {
  Ok(Json(views::reach(&node(&service, &name)?)?))
}

pub async fn target(service: Shared, Path(name): Path<String>) -> Answer {
  Ok(Json(views::target(&node(&service, &name)?)?))
}

pub async fn views(service: Shared) -> Answer {
  Ok(Json(json!({ "nodes": all_views(&service)? })))
}

#[derive(Deserialize)]
struct Check {
  #[serde(default)]
  name: Option<String>,
}

/// The one node named, or every node, checked now and written to its row; then the list as the console
/// views it.
pub async fn check(service: Shared, bytes: Bytes) -> Answer {
  let asked: Check = body(&bytes)?;
  let checked = check_nodes(&*service.store, &service.machine, asked.name.as_deref()).await?;

  if checked.changed {
    changed(&service);
  }

  Ok(Json(json!({ "nodes": all_views(&service)? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Added {
  name: String,
  host: String,
  user: String,
  #[serde(default)]
  port: Option<i64>,
  #[serde(default)]
  key_path: Option<String>,
  #[serde(default)]
  shell: Option<String>,
  #[serde(default)]
  label: Option<String>,
  #[serde(default)]
  role: Option<String>,
}

pub async fn add(service: Shared, bytes: Bytes) -> Answer {
  let asked: Added = body(&bytes)?;
  let login = SshLogin {
    host: asked.host,
    port: asked.port.unwrap_or(DEFAULT_SSH_PORT),
    user: asked.user,
    key_path: asked.key_path,
    shell: asked.shell.unwrap_or_else(|| DEFAULT_LOGIN_SHELL.to_owned()),
  };
  let node = service.store.add_node(&asked.name, asked.label.as_deref(), asked.role, login)?;

  changed(&service);
  Ok(Json(views::view(&node, &service.here)))
}

/// A text field a change names, null where `clears` lets it be cleared.
fn text(fields: &Map<String, Value>, name: &str) -> Result<Option<Option<String>>, Reply> {
  match fields.get(name) {
    None => Ok(None),
    Some(Value::Null) => Ok(Some(None)),
    Some(Value::String(text)) => Ok(Some(Some(text.clone()))),
    Some(_) => Err(Reply::bad_request(format!("{name} is text"))),
  }
}

fn required_text(fields: &Map<String, Value>, name: &str) -> Result<Option<String>, Reply> {
  match text(fields, name)? {
    Some(None) => Err(Reply::bad_request(format!("{name} cannot be cleared"))),
    other => Ok(other.flatten()),
  }
}

pub async fn set(service: Shared, Path(name): Path<String>, bytes: Bytes) -> Answer {
  let fields: Map<String, Value> = body(&bytes)?;
  let port = match fields.get("port") {
    None => None,
    Some(value) => Some(value.as_i64().ok_or_else(|| Reply::bad_request("port is a whole number"))?),
  };
  let changes = NodeChanges {
    label: required_text(&fields, "label")?,
    role: text(&fields, "role")?,
    host: required_text(&fields, "host")?,
    user: required_text(&fields, "user")?,
    port,
    key_path: text(&fields, "keyPath")?,
    shell: required_text(&fields, "shell")?,
  };
  let node = service.store.update_node(&name, changes)?;

  changed(&service);
  Ok(Json(views::view(&node, &service.here)))
}

pub async fn remove(service: Shared, Path(name): Path<String>) -> Answer {
  if !service.store.remove_node(&name)? {
    return Err(Reply::no_node(&name));
  }

  changed(&service);
  Ok(Json(json!({ "removed": name })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubAsked {
  url: String,
  token_file: String,
}

pub async fn add_hub(service: Shared, bytes: Bytes) -> Answer {
  let asked: HubAsked = body(&bytes)?;
  let hub = service.store.add_hub(&asked.url, &asked.token_file)?;

  changed(&service);
  Ok(Json(json!({ "url": hub.url, "tokenFile": hub.token_file })))
}

pub async fn remove_hub(service: Shared, Query(query): Query<HashMap<String, String>>) -> Answer {
  let url = query.get("url").ok_or_else(|| Reply::bad_request("the query names no url"))?;
  let Some(removed) = service.store.remove_hub(url)? else {
    return Err(Reply::refused("no-hub", format!("no hub is listed at {url}")));
  };

  changed(&service);
  Ok(Json(json!({ "url": store::nodes::checked_hub_url(url)?, "removed": removed })))
}
