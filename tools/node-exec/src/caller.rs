//! Who may run a command on which node (docs/tools.md, 「Who may run a command on which node」): a caller
//! reaches the nodes its own account owns, and a session holding the administrator's grant reaches every
//! node. The caller's account and the node's owner are read from agency, the issuer of the session records,
//! and so is the node a call names by its account (docs/tools.md, 「A call」).

use std::path::Path;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The resource a session's grants hold where it is the administrator's.
pub const ADMIN_GRANT: &str = "node.admin";

// Agency's control port where nothing names another (agency's docs/api.md, 「The control port」).
const AGENCY_PORT: &str = "3220";

/// One grant of a session record.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Grant {
  #[serde(default)]
  pub resource: String,
}

/// The parts of the session record the call is made under that the tool reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
  #[serde(default)]
  pub holder_name: String,
  #[serde(default)]
  pub grants: Vec<Grant>,
  #[serde(default)]
  pub labels: Map<String, Value>,
}

impl Session {
  /// Whether the record holds the administrator's grant.
  pub fn is_administrator(&self) -> bool {
    self.grants.iter().any(|grant| grant.resource == ADMIN_GRANT)
  }

  /// The agent the call is made for: the supervisor a double stands for, or the record's holder.
  pub fn agent(&self) -> &str {
    self.labels.get("doubleOf").and_then(Value::as_str).unwrap_or(&self.holder_name)
  }
}

/// Agency's control port, asked who owns an agent and a node.
pub struct Agency {
  base: String,
  token: String,
  http: reqwest::Client,
}

/// An account's id written out, as agency writes it and a call names it: a number or a string alike.
pub fn account(value: Option<&Value>) -> Option<String> {
  match value? {
    Value::Number(number) => Some(number.to_string()),
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    _ => None,
  }
}

fn items(body: &Value) -> &[Value] {
  body.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

impl Agency {
  pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
    Self {
      base: base.into().trim_end_matches('/').to_owned(),
      token: token.into(),
      http: reqwest::Client::new(),
    }
  }

  /// Agency's control port and token as the data root's `env/agency.env` names them, a variable of this
  /// process's own environment standing first.
  pub fn read(data_root: &Path) -> Result<Self, String> {
    let file = data_root.join("env/agency.env");
    let mut pairs: Vec<(String, String)> = match dotenvy::from_path_iter(&file) {
      Ok(pairs) => pairs.filter_map(Result::ok).collect(),
      Err(_) => Vec::new(),
    };

    pairs.extend(std::env::vars());

    let named =
      |key: &str| pairs.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value.clone());
    let port = named("AGENCY_CONTROL_PORT").unwrap_or_else(|| AGENCY_PORT.to_owned());
    let token = match named("AGENCY_CONTROL_TOKEN") {
      Some(token) => token,
      None => {
        let path = named("AGENCY_CONTROL_TOKEN_FILE")
          .ok_or_else(|| format!("{} names no agency control token", file.display()))?;

        std::fs::read_to_string(&path)
          .map_err(|_| format!("agency's control token file {path} cannot be read"))?
          .trim()
          .to_owned()
      }
    };

    Ok(Self::new(format!("http://127.0.0.1:{port}"), token))
  }

  async fn get(&self, path: &str) -> Result<(u16, Value), String> {
    let answer = self
      .http
      .get(format!("{}{path}", self.base))
      .bearer_auth(&self.token)
      .send()
      .await
      .map_err(|why| format!("agency did not answer: {why}"))?;
    let status = answer.status().as_u16();
    let body = answer.json::<Value>().await.unwrap_or(Value::Null);

    Ok((status, body))
  }

  async fn read_ok(&self, path: &str) -> Result<Value, String> {
    match self.get(path).await? {
      (200, body) => Ok(body),
      (status, _) => Err(format!("agency answered {status} to {path}")),
    }
  }

  /// The account an agent belongs to: a supervisor's owner, else a role agent's; none where agency knows it
  /// as neither or it names no owner.
  pub async fn account_of(&self, agent: &str) -> Result<Option<String>, String> {
    if agent.is_empty() {
      return Ok(None);
    }

    let path = format!("/supervisors/{}", utf8_percent_encode(agent, NON_ALPHANUMERIC));

    match self.get(&path).await? {
      (200, body) => return Ok(account(body.get("ownerAccount"))),
      (404, _) => {}
      (status, _) => return Err(format!("agency answered {status} to {path}")),
    }

    let agents = self.read_ok("/role-agents").await?;
    let named = items(&agents).iter().find(|one| one.get("name").and_then(Value::as_str) == Some(agent));

    Ok(named.and_then(|one| account(one.get("ownerAccount"))))
  }

  /// The account agency's record of a person's node names for `node`, or none where it is no person's node.
  pub async fn owner_of(&self, node: &str) -> Result<Option<String>, String> {
    let records = self.read_ok("/person-nodes").await?;
    let record = items(&records).iter().find(|one| one.get("node").and_then(Value::as_str) == Some(node));

    Ok(record.and_then(|one| account(one.get("ownerAccount"))))
  }

  /// The node agency's record of a person's node names for `account`, or none where no record names one.
  pub async fn node_of(&self, account_id: &str) -> Result<Option<String>, String> {
    let records = self.read_ok("/person-nodes").await?;
    let record = items(&records)
      .iter()
      .find(|one| account(one.get("ownerAccount")).as_deref() == Some(account_id));

    Ok(record.and_then(|one| one.get("node").and_then(Value::as_str)).map(str::to_owned))
  }
}

/// The node a call names by its account, or the reason it cannot be had, naming the account.
pub async fn node_of(account_id: &str, agency: Result<&Agency, &String>) -> Result<String, String> {
  let unread = |why: &str| format!("Which node account {account_id} has could not be read: {why}");
  let agency = agency.map_err(|why| unread(why))?;

  agency.node_of(account_id).await.map_err(|why| unread(&why))?.ok_or_else(|| {
    format!("Account {account_id} has no node: no record of agency's names a person's node for it.")
  })
}

/// Whether the session may run a command on `node`, or the reason it may not, naming the node and its
/// owner. An owner or an account that cannot be read lets nobody through but the administrator.
pub async fn judge(session: &Session, node: &str, agency: Result<&Agency, &String>) -> Result<(), String> {
  if session.is_administrator() {
    return Ok(());
  }

  let unread = |why: &str| {
    format!(
      "Whose node {node} is could not be read, and the session holds no administrator's grant ({ADMIN_GRANT}): {why}"
    )
  };
  let agency = agency.map_err(|why| unread(why))?;
  let owner = agency.owner_of(node).await.map_err(|why| unread(&why))?;
  let agent = session.agent();
  let caller = agency.account_of(agent).await.map_err(|why| unread(&why))?;

  if owner.is_some() && owner == caller {
    return Ok(());
  }

  let whose = match &owner {
    Some(owner) => format!("The node {node} is account {owner}'s"),
    None => format!("The node {node} is no person's node and is the administrator's alone"),
  };
  let who = match &caller {
    Some(caller) => format!("this session's agent {agent} is account {caller}'s"),
    None => format!("this session's agent {agent} belongs to no account"),
  };

  Err(format!("{whose}; {who} and holds no administrator's grant ({ADMIN_GRANT})."))
}
