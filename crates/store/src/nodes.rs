//! The node list and the hubs, read and changed (docs/nodes.md, 「What a row holds」 and 「Registering and
//! changing a node」). A node is always read with its hub, in one query, so its kind, its agent and its hub's
//! id and token file come together. No row holds a credential, only where one is kept.

use bmo_node_client::ErrorCode;
use policy::node_states::NodeState;
use policy::nodes::{MonitoredAccounts, NodeKind, Ownership, is_account, is_node_name, personal_account};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Result, Store, StoreError, now_seconds};

/// The port an ssh node that names none is logged in to.
pub const DEFAULT_SSH_PORT: i64 = 22;

/// The languages an ssh login answers a command in: a POSIX shell, or Windows PowerShell.
pub const LOGIN_SHELLS: [&str; 2] = ["posix", "powershell"];

/// The language a login that names none answers in.
pub const DEFAULT_LOGIN_SHELL: &str = "posix";

/// The most characters a label holds, counted in UTF-16 units as a browser counts them.
pub const LABEL_MOST: usize = 48;

/// One node as the list holds it, with its hub's id and token file beside a hub node.
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
  /// A hub node's hub's id; none where the hub is not listed.
  pub hub_id: Option<i64>,
  /// A hub node's hub's token file; none where the hub is not listed.
  pub token_file: Option<String>,
  /// A machine of the platform, or a person's own PC (docs/nodes.md, 「A node's ownership」).
  pub ownership: Ownership,
  /// The account a personal node belongs to, its id written out; none on a shared node.
  pub account: Option<String>,
  pub role: Option<String>,
  /// The callers the node lets make a run on it, joined by `+`; none where it lets none.
  pub callers: Option<String>,
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

  /// The callers the node lets make a run on it, by name.
  pub fn callers(&self) -> Vec<&str> {
    self.callers.as_deref().map(|callers| callers.split('+').collect()).unwrap_or_default()
  }

  /// Whether the hardware monitor reads the node: a shared node, or a personal one of an account among those
  /// monitored, never another person's own PC (docs/hardware.md, 「Which machines」).
  pub fn monitored(&self, accounts: &MonitoredAccounts) -> bool {
    accounts.read(self.ownership, self.account.as_deref())
  }

  /// Whether the node lets `caller` make a run on it.
  pub fn lets(&self, caller: &str) -> bool {
    self.callers().contains(&caller)
  }

  /// Where the node is reached, as one line a person reads. It names no credential.
  pub fn address(&self) -> String {
    if self.kind == NodeKind::Hub {
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

  /// Where the node's credential is kept: an ssh node's key path, or the token file of a hub node's hub.
  pub fn credential(&self) -> Option<&str> {
    match self.kind {
      NodeKind::Ssh => self.key_path.as_deref(),
      NodeKind::Hub => self.token_file.as_deref(),
    }
  }
}

/// One hub: its id, its control address, the file its token stands in, its label and when it was listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hub {
  pub id: i64,
  pub url: String,
  pub token_file: String,
  pub label: Option<String>,
  pub created_at: i64,
}

/// A hub taken off the list, with the names of the nodes reached through it, removed with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubRemoved {
  pub url: String,
  pub removed: Vec<String>,
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

/// What a change may say: a field left out stays as it is, and a role, an account or a key of none clears
/// it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeChanges {
  pub label: Option<String>,
  pub role: Option<Option<String>>,
  pub ownership: Option<Ownership>,
  pub account: Option<Option<String>>,
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

// Every node with its hub's id and token file beside it: the one query a node is read by.
const NODE_SELECT: &str = "SELECT nodes.name, nodes.label, nodes.kind, nodes.host, nodes.port, nodes.user,
  nodes.key_path, nodes.shell, nodes.agent, nodes.hub_url, hubs.token_file, nodes.role, nodes.position,
  nodes.state, nodes.detail, nodes.checked_at, nodes.connected_at, hubs.id, nodes.callers, nodes.ownership,
  nodes.account
  FROM nodes LEFT JOIN hubs ON hubs.url = nodes.hub_url";

const HUB_SELECT: &str = "SELECT id, url, token_file, label, created_at FROM hubs";

fn hub_of(row: &Row<'_>) -> rusqlite::Result<Hub> {
  Ok(Hub {
    id: row.get(0)?,
    url: row.get(1)?,
    token_file: row.get(2)?,
    label: row.get(3)?,
    created_at: row.get(4)?,
  })
}

fn node_of(row: &Row<'_>) -> rusqlite::Result<Node> {
  let kind: String = row.get(2)?;
  let shell: Option<String> = row.get(7)?;
  let state: String = row.get(13)?;
  let ownership: String = row.get(19)?;

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
    hub_id: row.get(17)?,
    token_file: row.get(10)?,
    ownership: Ownership::of_code(&ownership).unwrap_or_default(),
    account: row.get(20)?,
    role: row.get(11)?,
    callers: row.get(18)?,
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

fn refused(code: ErrorCode, message: impl Into<String>) -> StoreError {
  StoreError::refused(code, message)
}

fn no_node(name: &str) -> StoreError {
  refused(ErrorCode::NotFound, format!("no node named {name}"))
}

fn checked_label(label: &str) -> Result<String> {
  let trimmed = label.trim();

  if trimmed.is_empty() || trimmed.contains('\n') {
    return Err(refused(ErrorCode::BadField, "a label is one line of words"));
  }

  if trimmed.encode_utf16().count() > LABEL_MOST {
    return Err(refused(ErrorCode::BadField, format!("a label is at most {LABEL_MOST} characters")));
  }

  Ok(trimmed.to_owned())
}

fn checked_word(value: &str, what: &str) -> Result<String> {
  let trimmed = value.trim();

  if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
    return Err(refused(ErrorCode::BadField, format!("{what} is one word with no spaces")));
  }

  Ok(trimmed.to_owned())
}

fn checked_callers(callers: Option<String>) -> Result<Option<String>> {
  match callers {
    Some(callers) if !callers.split('+').all(is_node_name) => Err(refused(
      ErrorCode::BadField,
      format!(
        "callers name one caller, or several joined by +, each lowercase letters, digits and hyphens, not \"{callers}\""
      ),
    )),
    callers => Ok(callers),
  }
}

fn checked_role(role: Option<String>) -> Result<Option<String>> {
  match role {
    Some(role) if !role.split('+').all(is_node_name) => Err(refused(
      ErrorCode::BadField,
      format!(
        "a role names one service, or several joined by +, each lowercase letters, digits and hyphens, not \"{role}\""
      ),
    )),
    role => Ok(role),
  }
}

/// A node's ownership and account once a change is laid over them (docs/nodes.md, 「A node's ownership」): a
/// node made or left shared holds no account, and an account is a decimal number.
fn checked_ownership(node: &Node, changes: &NodeChanges) -> Result<(Ownership, Option<String>)> {
  let ownership = changes.ownership.unwrap_or(node.ownership);

  match (ownership, changes.account.clone()) {
    (Ownership::Shared, Some(Some(_))) => Err(refused(
      ErrorCode::BadField,
      format!("{} is shared, and only a personal node belongs to an account", node.name),
    )),
    (Ownership::Shared, _) => Ok((ownership, None)),
    (Ownership::Personal, Some(Some(account))) if !is_account(&account) => Err(refused(
      ErrorCode::BadField,
      format!("an account is the account's id as a decimal number, not \"{account}\""),
    )),
    (Ownership::Personal, None) => Ok((ownership, node.account.clone())),
    (Ownership::Personal, Some(account)) => Ok((ownership, account)),
  }
}

/// A hub's control address held to one form: an http or https address with no trailing slash.
pub fn checked_hub_url(value: &str) -> Result<String> {
  let refusal = || {
    refused(
      ErrorCode::BadField,
      format!("a hub's control address is an http address, not \"{value}\""),
    )
  };
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
      ErrorCode::BadField,
      format!("a port is a whole number from 1 to 65535, not {}", login.port),
    ));
  }

  if !LOGIN_SHELLS.contains(&login.shell.as_str()) {
    return Err(refused(
      ErrorCode::BadField,
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

/// The name an agent's node takes: the agent's name held to the node-name rule, every other run of
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
    let mut query = connection.prepare(&format!("{HUB_SELECT} ORDER BY id"))?;
    let hubs = query.query_map([], hub_of)?.collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(hubs)
  }

  /// One hub by its id.
  pub fn hub(&self, id: i64) -> Result<Option<Hub>> {
    Ok(
      self
        .connection()
        .query_row(&format!("{HUB_SELECT} WHERE id = ?1"), [id], hub_of)
        .optional()?,
    )
  }

  /// One hub by its control address, held to the one form every address is kept in.
  pub fn hub_at(&self, url: &str) -> Result<Option<Hub>> {
    let url = checked_hub_url(url)?;

    Ok(
      self
        .connection()
        .query_row(&format!("{HUB_SELECT} WHERE url = ?1"), [&url], hub_of)
        .optional()?,
    )
  }

  /// A hub listed, with a label where one is given. A hub already listed is refused.
  pub fn add_hub(&self, url: &str, token_file: &str, label: Option<&str>) -> Result<Hub> {
    let url = checked_hub_url(url)?;
    let token_file = checked_word(token_file, "a token file")?;
    let label = label.map(checked_label).transpose()?;
    let connection = self.connection();
    let listed: bool =
      connection
        .query_row("SELECT EXISTS (SELECT 1 FROM hubs WHERE url = ?1)", [&url], |row| row.get(0))?;

    if listed {
      return Err(refused(ErrorCode::Conflict, format!("the hub {url} is already listed")));
    }

    connection.execute(
      "INSERT INTO hubs (url, token_file, label, created_at) VALUES (?1, ?2, ?3, ?4)",
      params![url, token_file, label, now_seconds()],
    )?;

    Ok(connection.query_row(&format!("{HUB_SELECT} WHERE id = last_insert_rowid()"), [], hub_of)?)
  }

  /// A hub taken off the list with every node reached through it, since none of them can be reached
  /// without it: its address and the names of the nodes removed, or none where no hub has that id.
  pub fn remove_hub(&self, id: i64) -> Result<Option<HubRemoved>> {
    let mut connection = self.connection();
    let unit = connection.transaction()?;
    let Some(url) = unit
      .query_row("SELECT url FROM hubs WHERE id = ?1", [id], |row| row.get::<_, String>(0))
      .optional()?
    else {
      return Ok(None);
    };
    let removed = {
      let mut query = unit
        .prepare("SELECT name FROM nodes WHERE kind = 'hub' AND hub_url = ?1 ORDER BY position, name")?;
      query
        .query_map([&url], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    unit.execute("DELETE FROM nodes WHERE kind = 'hub' AND hub_url = ?1", [&url])?;
    unit.execute("DELETE FROM hubs WHERE id = ?1", [id])?;
    unit.commit()?;

    Ok(Some(HubRemoved { url, removed }))
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
        ErrorCode::NameRule,
        format!("a node's name is lowercase letters, digits and hyphens, at most 32, not \"{name}\""),
      ));
    }

    let role = checked_role(role)?;
    let label = checked_label(label.unwrap_or(name))?;
    let login = checked_login(login)?;
    let connection = self.connection();

    if exists_on(&connection, name)? {
      return Err(refused(ErrorCode::Conflict, format!("a node named {name} already exists")));
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

  /// A node changed. The connection type never changes, and a hub node takes a label and a role alone; a
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
    let (ownership, account) = checked_ownership(&node, &changes)?;

    if node.kind == NodeKind::Hub {
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
          ErrorCode::BadField,
          format!(
            "{} is not a field of a hub node, which takes a label, a role, an ownership and an account alone",
            fields.join(", ")
          ),
        ));
      }

      connection.execute(
        "UPDATE nodes SET label = ?1, role = ?2, ownership = ?3, account = ?4, updated_at = ?5 WHERE name = ?6",
        params![label, role, ownership.name(), account, now_seconds(), name],
      )?;

      return get_on(&connection, name)?.ok_or_else(|| no_node(name));
    }

    let (Some(host), Some(user)) = (node.host.clone(), node.user.clone()) else {
      return Err(refused(
        ErrorCode::Incomplete,
        format!("{name} is missing a field its connection type needs"),
      ));
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
         ownership = ?8, account = ?9, updated_at = ?10{forget} WHERE name = ?11"
      ),
      params![
        label,
        role,
        login.host,
        login.port,
        login.user,
        login.key_path,
        login.shell,
        ownership.name(),
        account,
        now_seconds(),
        name
      ],
    )?;

    get_on(&connection, name)?.ok_or_else(|| no_node(name))
  }

  /// The callers a node lets make a run on it, set whole, or cleared by none (docs/nodes.md, 「A node's
  /// callers」). Only the command writes them.
  pub fn set_callers(&self, name: &str, callers: Option<String>) -> Result<Node> {
    let callers = checked_callers(callers)?;
    let connection = self.connection();
    let changed = connection.execute(
      "UPDATE nodes SET callers = ?1, updated_at = ?2 WHERE name = ?3",
      params![callers, now_seconds(), name],
    )?;

    if changed == 0 {
      return Err(no_node(name));
    }

    get_on(&connection, name)?.ok_or_else(|| no_node(name))
  }

  /// A node taken off the list; whether there was one.
  pub fn remove_node(&self, name: &str) -> Result<bool> {
    Ok(self.connection().execute("DELETE FROM nodes WHERE name = ?1", [name])? > 0)
  }

  /// The hub node for an agent attached to a hub, added last in the list where none stands for it yet,
  /// named after the agent and labelled with its name, and personal where the name is a person's machine's
  /// (docs/nodes.md, 「A node's ownership」), a personal one naming the callers the store was given for
  /// personal nodes (「A node's callers」); whether one was added.
  pub fn add_agent(&self, hub: &str, agent: &str) -> Result<bool> {
    let connection = self.connection();
    let known: bool = connection.query_row(
      "SELECT EXISTS (SELECT 1 FROM nodes WHERE kind = 'hub' AND hub_url = ?1 AND agent = ?2)",
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
    let account = personal_account(agent);
    let ownership = if account.is_some() { Ownership::Personal } else { Ownership::Shared };
    let callers = account.as_ref().and_then(|_| self.personal_callers.joined());

    connection.execute(
      "INSERT INTO nodes (name, label, kind, agent, hub_url, ownership, account, callers, position, created_at, updated_at)
       VALUES (?1, ?2, 'hub', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
      params![name, label, agent, hub, ownership.name(), account, callers, next_position(&connection)?, at],
    )?;

    Ok(true)
  }

  /// The shell an agent's host takes a command in, as its hub says, written to its hub node where it differs.
  pub fn set_agent_shell(&self, hub: &str, agent: &str, shell: &str) -> Result<()> {
    self.connection().execute(
      "UPDATE nodes SET shell = ?3 WHERE kind = 'hub' AND hub_url = ?1 AND agent = ?2 AND shell IS DISTINCT FROM ?3",
      params![hub, agent, shell],
    )?;

    Ok(())
  }

  /// The hub node of an agent its hub says has departed, removed; whether there was one.
  pub fn remove_agent(&self, hub: &str, agent: &str) -> Result<bool> {
    Ok(
      self
        .connection()
        .execute("DELETE FROM nodes WHERE kind = 'hub' AND hub_url = ?1 AND agent = ?2", [hub, agent])?
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
