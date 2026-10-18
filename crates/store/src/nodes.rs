//! The node list and the hubs, read and changed (docs/nodes.md, 「What a row holds」 and 「Registering and
//! changing a node」). A node is always read with its hub, in one query, so its kind, its agent and its hub's
//! token file come together. No row holds a credential, only where one is kept.

use policy::node_states::NodeState;
use policy::nodes::{NodeKind, is_node_name};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Result, Store, StoreError, now_seconds};

/// The port an ssh node that names none is logged in to.
pub const DEFAULT_SSH_PORT: i64 = 22;

/// The languages an ssh login answers a command in: a POSIX shell, or Windows PowerShell.
pub const LOGIN_SHELLS: [&str; 2] = ["posix", "powershell"];

/// The language a login that names none answers in.
pub const DEFAULT_LOGIN_SHELL: &str = "posix";

/// The most characters a label holds, as the console counts them.
pub const LABEL_MOST: usize = 48;

/// The service a node's role names to carry a worker's jobs and heavy runs.
pub const JOBS_SERVICE: &str = "jobs";

/// One node as the list holds it, with its hub's token file beside a jake node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
  pub name: String,
  pub label: String,
  pub kind: NodeKind,
  pub host: Option<String>,
  pub port: Option<i64>,
  pub user: Option<String>,
  pub key_path: Option<String>,
  /// The language the login answers in, `posix` where the row names none.
  pub shell: String,
  pub agent: Option<String>,
  pub hub_url: Option<String>,
  /// A jake node's hub's token file; none where the hub is not listed.
  pub token_file: Option<String>,
  pub role: Option<String>,
  pub position: i64,
  pub state: NodeState,
  pub detail: Option<String>,
  pub checked_at: Option<i64>,
  pub connected_at: Option<i64>,
}

impl Node {
  /// The port the node is logged in to.
  pub fn ssh_port(&self) -> i64 {
    self.port.unwrap_or(DEFAULT_SSH_PORT)
  }

  /// Whether the node's login answers in Windows PowerShell.
  pub fn powershell(&self) -> bool {
    self.shell == "powershell"
  }

  /// The services the node's role names.
  pub fn services(&self) -> Vec<&str> {
    self.role.as_deref().map(|role| role.split('+').collect()).unwrap_or_default()
  }

  /// Where the node is reached, as one line a person reads. It names no credential.
  pub fn address(&self) -> String {
    if self.kind == NodeKind::Jake {
      return format!(
        "{} @ {}",
        self.agent.as_deref().unwrap_or_default(),
        self.hub_url.as_deref().unwrap_or_default()
      );
    }

    let port = self.port.filter(|port| *port != DEFAULT_SSH_PORT).map(|port| format!(":{port}"));
    let shell = if self.powershell() { " (PowerShell)" } else { "" };

    format!(
      "{}@{}{}{shell}",
      self.user.as_deref().unwrap_or_default(),
      self.host.as_deref().unwrap_or_default(),
      port.unwrap_or_default()
    )
  }

  /// Where the node's credential is kept: an ssh node's key path, or the token file of a jake node's hub.
  pub fn credential(&self) -> Option<&str> {
    match self.kind {
      NodeKind::Ssh => self.key_path.as_deref(),
      NodeKind::Jake => self.token_file.as_deref(),
    }
  }
}

/// One hub: its control address and the file its token stands in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hub {
  pub url: String,
  pub token_file: String,
}

/// An ssh node's login as a person registers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshLogin {
  pub host: String,
  pub port: i64,
  pub user: String,
  pub key_path: Option<String>,
  pub shell: String,
}

impl SshLogin {
  /// A login on port 22, offering the account's own SSH setup, answering in a POSIX shell.
  pub fn new(host: impl Into<String>, user: impl Into<String>) -> Self {
    Self {
      host: host.into(),
      port: DEFAULT_SSH_PORT,
      user: user.into(),
      key_path: None,
      shell: DEFAULT_LOGIN_SHELL.to_owned(),
    }
  }
}

/// What a change may say: a field left out stays as it is, and a role or a key of none clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeChanges {
  pub label: Option<String>,
  pub role: Option<Option<String>>,
  pub host: Option<String>,
  pub user: Option<String>,
  pub port: Option<i64>,
  pub key_path: Option<Option<String>>,
  pub shell: Option<String>,
}

/// What a check of a node came to, as its row keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
  pub state: NodeState,
  pub detail: Option<String>,
  pub at: i64,
}

// Every node with its hub's token file beside it: the one query a node is read by.
const NODE_SELECT: &str = "SELECT nodes.name, nodes.label, nodes.kind, nodes.host, nodes.port, nodes.user,
  nodes.key_path, nodes.shell, nodes.agent, nodes.hub_url, hubs.token_file, nodes.role, nodes.position,
  nodes.state, nodes.detail, nodes.checked_at, nodes.connected_at
  FROM nodes LEFT JOIN hubs ON hubs.url = nodes.hub_url";

fn node_of(row: &Row<'_>) -> rusqlite::Result<Node> {
  let kind: String = row.get(2)?;
  let shell: Option<String> = row.get(7)?;
  let state: String = row.get(13)?;

  Ok(Node {
    name: row.get(0)?,
    label: row.get(1)?,
    kind: NodeKind::of_code(&kind).unwrap_or(NodeKind::Ssh),
    host: row.get(3)?,
    port: row.get(4)?,
    user: row.get(5)?,
    key_path: row.get(6)?,
    shell: shell
      .filter(|shell| LOGIN_SHELLS.contains(&shell.as_str()))
      .unwrap_or_else(|| DEFAULT_LOGIN_SHELL.to_owned()),
    agent: row.get(8)?,
    hub_url: row.get(9)?,
    token_file: row.get(10)?,
    role: row.get(11)?,
    position: row.get(12)?,
    state: NodeState::of_column(&state),
    detail: row.get(14)?,
    checked_at: row.get(15)?,
    connected_at: row.get(16)?,
  })
}

fn get_on(connection: &Connection, name: &str) -> rusqlite::Result<Option<Node>> {
  connection
    .query_row(&format!("{NODE_SELECT} WHERE nodes.name = ?1"), [name], node_of)
    .optional()
}

fn exists_on(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
  connection.query_row("SELECT EXISTS (SELECT 1 FROM nodes WHERE name = ?1)", [name], |row| row.get(0))
}

fn next_position(connection: &Connection) -> rusqlite::Result<i64> {
  connection.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM nodes", [], |row| row.get(0))
}

fn refused(code: &'static str, message: impl Into<String>) -> StoreError {
  StoreError::refused(code, message)
}

fn no_node(name: &str) -> StoreError {
  refused("no-node", format!("no node named {name}"))
}

fn checked_label(label: &str) -> Result<String> {
  let trimmed = label.trim();

  if trimmed.is_empty() || trimmed.contains('\n') {
    return Err(refused("bad-field", "a label is one line of words"));
  }

  if trimmed.encode_utf16().count() > LABEL_MOST {
    return Err(refused("bad-field", format!("a label is at most {LABEL_MOST} characters")));
  }

  Ok(trimmed.to_owned())
}

fn checked_word(value: &str, what: &str) -> Result<String> {
  let trimmed = value.trim();

  if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
    return Err(refused("bad-field", format!("{what} is one word with no spaces")));
  }

  Ok(trimmed.to_owned())
}

fn checked_role(role: Option<String>) -> Result<Option<String>> {
  match role {
    Some(role) if !role.split('+').all(is_node_name) => Err(refused(
      "bad-field",
      format!(
        "a role names one service, or several joined by +, each lowercase letters, digits and hyphens, not \"{role}\""
      ),
    )),
    role => Ok(role),
  }
}

/// A hub's control address held to one form: an http or https address with no trailing slash.
pub fn checked_hub_url(value: &str) -> Result<String> {
  let refusal =
    || refused("bad-field", format!("a hub's control address is an http address, not \"{value}\""));
  let url = url::Url::parse(value.trim()).map_err(|_| refusal())?;

  if !matches!(url.scheme(), "http" | "https") {
    return Err(refusal());
  }

  let text = url.to_string();

  Ok(text.strip_suffix('/').map(str::to_owned).unwrap_or(text))
}

fn checked_login(login: SshLogin) -> Result<SshLogin> {
  if !(1..=65_535).contains(&login.port) {
    return Err(refused(
      "bad-field",
      format!("a port is a whole number from 1 to 65535, not {}", login.port),
    ));
  }

  if !LOGIN_SHELLS.contains(&login.shell.as_str()) {
    return Err(refused(
      "bad-field",
      format!("a login shell is one of {}, not \"{}\"", LOGIN_SHELLS.join(", "), login.shell),
    ));
  }

  Ok(SshLogin {
    host: checked_word(&login.host, "a host")?,
    port: login.port,
    user: checked_word(&login.user, "an account")?,
    key_path: login.key_path.map(|key| checked_word(&key, "a key path")).transpose()?,
    shell: login.shell,
  })
}

/// The name a jake agent's node takes: the agent's name held to the node-name rule, every other run of
/// characters a hyphen.
pub fn agent_node_name(agent: &str) -> String {
  let mut name = String::new();
  let mut replaced = false;

  for one in agent.to_lowercase().chars() {
    if one.is_ascii_lowercase() || one.is_ascii_digit() || one == '-' {
      name.push(one);
      replaced = false;
    } else if !replaced {
      name.push('-');
      replaced = true;
    }
  }

  let kept: String = name.trim_start_matches('-').chars().take(policy::nodes::NAME_MAX).collect();
  let kept = kept.trim_end_matches('-');

  if kept.is_empty() { "jake".to_owned() } else { kept.to_owned() }
}

impl Store {
  /// Every node, in the list's order.
  pub fn nodes(&self) -> Result<Vec<Node>> {
    let connection = self.connection();
    let mut query = connection.prepare(&format!("{NODE_SELECT} ORDER BY nodes.position, nodes.name"))?;
    let nodes = query.query_map([], node_of)?.collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(nodes)
  }

  /// One node by its name.
  pub fn node(&self, name: &str) -> Result<Option<Node>> {
    Ok(get_on(&self.connection(), name)?)
  }

  /// Whether a node of that name is listed.
  pub fn node_exists(&self, name: &str) -> Result<bool> {
    Ok(exists_on(&self.connection(), name)?)
  }

  /// Every hub, in the order they were listed.
  pub fn hubs(&self) -> Result<Vec<Hub>> {
    let connection = self.connection();
    let mut query = connection.prepare("SELECT url, token_file FROM hubs ORDER BY created_at, url")?;
    let hubs = query
      .query_map([], |row| {
        Ok(Hub {
          url: row.get(0)?,
          token_file: row.get(1)?,
        })
      })?
      .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(hubs)
  }

  /// A hub listed. A hub already listed is refused.
  pub fn add_hub(&self, url: &str, token_file: &str) -> Result<Hub> {
    let url = checked_hub_url(url)?;
    let token_file = checked_word(token_file, "a token file")?;
    let connection = self.connection();
    let listed: bool =
      connection
        .query_row("SELECT EXISTS (SELECT 1 FROM hubs WHERE url = ?1)", [&url], |row| row.get(0))?;

    if listed {
      return Err(refused("hub-taken", format!("the hub {url} is already listed")));
    }

    connection.execute(
      "INSERT INTO hubs (url, token_file, created_at) VALUES (?1, ?2, ?3)",
      params![url, token_file, now_seconds()],
    )?;

    Ok(Hub { url, token_file })
  }

  /// A hub taken off the list with every node reached through it, since none of them can be reached
  /// without it: the names of the nodes removed, or none where the hub was not listed.
  pub fn remove_hub(&self, url: &str) -> Result<Option<Vec<String>>> {
    let url = checked_hub_url(url)?;
    let mut connection = self.connection();
    let unit = connection.transaction()?;
    let removed = {
      let mut query = unit
        .prepare("SELECT name FROM nodes WHERE kind = 'jake' AND hub_url = ?1 ORDER BY position, name")?;
      query
        .query_map([&url], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    unit.execute("DELETE FROM nodes WHERE kind = 'jake' AND hub_url = ?1", [&url])?;

    let listed = unit.execute("DELETE FROM hubs WHERE url = ?1", [&url])? > 0;

    unit.commit()?;

    Ok(listed.then_some(removed))
  }

  /// An ssh node registered, last in the list.
  pub fn add_node(
    &self,
    name: &str,
    label: Option<&str>,
    role: Option<String>,
    login: SshLogin,
  ) -> Result<Node> {
    if !is_node_name(name) {
      return Err(refused(
        "name-rule",
        format!("a node's name is lowercase letters, digits and hyphens, at most 32, not \"{name}\""),
      ));
    }

    let role = checked_role(role)?;
    let label = checked_label(label.unwrap_or(name))?;
    let login = checked_login(login)?;
    let connection = self.connection();

    if exists_on(&connection, name)? {
      return Err(refused("name-taken", format!("a node named {name} already exists")));
    }

    let at = now_seconds();

    connection.execute(
      "INSERT INTO nodes (name, label, kind, host, port, user, key_path, shell, role, position, created_at, updated_at)
       VALUES (?1, ?2, 'ssh', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
      params![
        name,
        label,
        login.host,
        login.port,
        login.user,
        login.key_path,
        login.shell,
        role,
        next_position(&connection)?,
        at
      ],
    )?;

    get_on(&connection, name)?.ok_or_else(|| no_node(name))
  }

  /// A node changed. The connection type never changes, and a jake node takes a label and a role alone; a
  /// check made of another address, account, port, key or shell says nothing of the new one, so it is
  /// forgotten.
  pub fn update_node(&self, name: &str, changes: NodeChanges) -> Result<Node> {
    let connection = self.connection();
    let Some(node) = get_on(&connection, name)? else {
      return Err(no_node(name));
    };
    let label = changes.label.as_deref().map_or_else(|| Ok(node.label.clone()), checked_label)?;
    let role = match changes.role.clone() {
      None => node.role.clone(),
      Some(role) => checked_role(role)?,
    };

    if node.kind == NodeKind::Jake {
      let fields: Vec<&str> = [
        ("host", changes.host.is_some()),
        ("port", changes.port.is_some()),
        ("user", changes.user.is_some()),
        ("keyPath", changes.key_path.is_some()),
        ("shell", changes.shell.is_some()),
      ]
      .into_iter()
      .filter_map(|(field, given)| given.then_some(field))
      .collect();

      if !fields.is_empty() {
        return Err(refused(
          "bad-field",
          format!(
            "{} is not a field of a jake node, which takes a label and a role alone",
            fields.join(", ")
          ),
        ));
      }

      connection.execute(
        "UPDATE nodes SET label = ?1, role = ?2, updated_at = ?3 WHERE name = ?4",
        params![label, role, now_seconds(), name],
      )?;

      return get_on(&connection, name)?.ok_or_else(|| no_node(name));
    }

    let (Some(host), Some(user)) = (node.host.clone(), node.user.clone()) else {
      return Err(refused("incomplete", format!("{name} is missing a field its connection type needs")));
    };
    let current = SshLogin {
      host,
      port: node.ssh_port(),
      user,
      key_path: node.key_path.clone(),
      shell: node.shell.clone(),
    };
    let login = checked_login(SshLogin {
      host: changes.host.unwrap_or_else(|| current.host.clone()),
      port: changes.port.unwrap_or(current.port),
      user: changes.user.unwrap_or_else(|| current.user.clone()),
      key_path: changes.key_path.unwrap_or_else(|| current.key_path.clone()),
      shell: changes.shell.unwrap_or_else(|| current.shell.clone()),
    })?;
    let forget = if login == current {
      ""
    } else {
      ", state = 'unknown', detail = NULL, checked_at = NULL, connected_at = NULL"
    };

    connection.execute(
      &format!(
        "UPDATE nodes SET label = ?1, role = ?2, host = ?3, port = ?4, user = ?5, key_path = ?6, shell = ?7,
         updated_at = ?8{forget} WHERE name = ?9"
      ),
      params![
        label,
        role,
        login.host,
        login.port,
        login.user,
        login.key_path,
        login.shell,
        now_seconds(),
        name
      ],
    )?;

    get_on(&connection, name)?.ok_or_else(|| no_node(name))
  }

  /// A node taken off the list; whether there was one.
  pub fn remove_node(&self, name: &str) -> Result<bool> {
    Ok(self.connection().execute("DELETE FROM nodes WHERE name = ?1", [name])? > 0)
  }

  /// The jake node for an agent attached to a hub, added last in the list where none stands for it yet,
  /// named after the agent and labelled with its name; whether one was added.
  pub fn add_agent(&self, hub: &str, agent: &str) -> Result<bool> {
    let connection = self.connection();
    let known: bool = connection.query_row(
      "SELECT EXISTS (SELECT 1 FROM nodes WHERE kind = 'jake' AND hub_url = ?1 AND agent = ?2)",
      [hub, agent],
      |row| row.get(0),
    )?;

    if known {
      return Ok(false);
    }

    let base = agent_node_name(agent);
    let mut name = base.clone();
    let mut count = 2;

    while exists_on(&connection, &name)? {
      let suffix = format!("-{count}");
      let kept: String = base.chars().take(policy::nodes::NAME_MAX - suffix.len()).collect();

      name = format!("{}{suffix}", kept.trim_end_matches('-'));
      count += 1;
    }

    let words = agent.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = match words.chars().take(LABEL_MOST).collect::<String>() {
      label if label.is_empty() => name.clone(),
      label => label,
    };
    let at = now_seconds();

    connection.execute(
      "INSERT INTO nodes (name, label, kind, agent, hub_url, position, created_at, updated_at)
       VALUES (?1, ?2, 'jake', ?3, ?4, ?5, ?6, ?6)",
      params![name, label, agent, hub, next_position(&connection)?, at],
    )?;

    Ok(true)
  }

  /// The jake node of an agent its hub says has departed, removed; whether there was one.
  pub fn remove_agent(&self, hub: &str, agent: &str) -> Result<bool> {
    Ok(
      self
        .connection()
        .execute("DELETE FROM nodes WHERE kind = 'jake' AND hub_url = ?1 AND agent = ?2", [hub, agent])?
        > 0,
    )
  }

  /// A check written to its node's row. The moment of the last connection stands until another connection
  /// moves it; a node removed while its check was out is not written back.
  pub fn record_check(&self, name: &str, checked: &Checked) -> Result<()> {
    self.connection().execute(
      "UPDATE nodes SET state = ?1, detail = ?2, checked_at = ?3,
       connected_at = CASE WHEN ?1 = 'connected' THEN ?3 ELSE connected_at END
       WHERE name = ?4",
      params![checked.state.name(), checked.detail, checked.at, name],
    )?;

    Ok(())
  }
}
