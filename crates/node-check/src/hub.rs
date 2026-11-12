//! A jake node's check: its hub asked which agents are attached and which have departed, with the
//! control token read out of the hub's token file for that one request.

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rows::Hub;
use crate::{Machine, Met, NodeState, last_line};

// The key a hub's token file holds its control token under.
const TOKEN_KEY: &str = "JAKE_CONTROL_TOKEN";

/// What a hub said when asked which agents are attached to it, and which have departed — been
/// uninstalled, which a hub tells apart from merely dropping — or what every node reached through it
/// met when it could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HubAnswer {
  Attached {
    agents: Vec<String>,
    // A hub that keeps no record of departures names none.
    #[serde(default)]
    departed: Vec<String>,
  },
  Failed(Met),
}

impl HubAnswer {
  /// What a jake node's check came to, read off its hub's answer.
  pub fn state_of(&self, agent: &str) -> Met {
    match self {
      Self::Failed(met) => met.clone(),
      Self::Attached { agents, .. } if agents.iter().any(|one| one == agent) => Met::connected(),
      Self::Attached { departed, .. } if departed.iter().any(|one| one == agent) => {
        Met::new(NodeState::Disconnected, "the agent was uninstalled from its machine")
      }
      Self::Attached { .. } => Met::new(NodeState::Disconnected, "the agent is not attached to its hub"),
    }
  }

  /// The agents attached, where the hub could be asked.
  pub fn agents(&self) -> &[String] {
    match self {
      Self::Attached { agents, .. } => agents,
      Self::Failed(_) => &[],
    }
  }

  /// The agents departed, where the hub could be asked.
  pub fn departed(&self) -> &[String] {
    match self {
      Self::Attached { departed, .. } => departed,
      Self::Failed(_) => &[],
    }
  }
}

/// The control token out of a hub's token file, or what stood in the way of reading it.
pub fn hub_token(machine: &Machine, token_file: &str) -> Result<String, String> {
  let unread = || format!("the token file {token_file} cannot be read");
  let pairs = dotenvy::from_path_iter(machine.expand_home(token_file)).map_err(|_| unread())?;
  let mut token = None;

  for pair in pairs {
    let (key, value) = pair.map_err(|_| unread())?;

    if key == TOKEN_KEY {
      token = Some(value);
    }
  }

  token
    .filter(|token| !token.is_empty())
    .ok_or_else(|| format!("the token file {token_file} holds no {TOKEN_KEY}"))
}

/// Tells a hub to send its attached agent `agent` a leave: the agent uninstalls itself and departs (the hub's
/// `POST /leave`). The hub's own reason where it refuses, as it does for an agent not attached.
pub async fn leave(machine: &Machine, hub: &Hub, agent: &str) -> Result<(), String> {
  let token = hub_token(machine, &hub.token_file)?;
  let answer = machine
    .http
    .post(format!("{}/leave", hub.url))
    .query(&[("agent", agent)])
    .bearer_auth(token)
    .send()
    .await
    .map_err(|error| {
      format!(
        "the hub at {} did not answer: {}",
        hub.url,
        last_line(&error.to_string()).unwrap_or_default()
      )
    })?;
  let status = answer.status();

  if status.is_success() {
    return Ok(());
  }

  let said: serde_json::Value = answer.json().await.unwrap_or_default();
  let reason = said["reason"].as_str().map_or_else(|| status.to_string(), str::to_owned);

  Err(format!("the hub refused the leave of {agent}: {reason}"))
}

/// Asks a hub which agents are attached to it.
pub async fn ask(machine: &Machine, hub: &Hub) -> HubAnswer {
  let failed = |state, detail: String| HubAnswer::Failed(Met::new(state, detail));
  let token = match hub_token(machine, &hub.token_file) {
    Ok(token) => token,
    Err(detail) => return failed(NodeState::Error, detail),
  };
  let asked = machine.http.get(format!("{}/status", hub.url)).bearer_auth(token).send().await;
  let answer = match asked {
    Ok(answer) => answer,
    Err(error) => {
      return HubAnswer::Failed(Met {
        state: NodeState::Unreachable,
        detail: last_line(&error.to_string()),
      });
    }
  };
  let status = answer.status();

  if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
    return failed(NodeState::Auth, format!("the hub refused the token ({})", status.as_u16()));
  }

  if !status.is_success() {
    return failed(NodeState::Error, format!("the hub answered {}", status.as_u16()));
  }

  let said: Option<Value> = answer.json().await.ok();
  let Some(agents) = said.as_ref().and_then(|said| said.get("agents")).and_then(Value::as_array) else {
    return failed(NodeState::Error, "the hub's status could not be read".into());
  };

  let names = |list: &[Value]| -> Vec<String> {
    list
      .iter()
      .filter_map(|agent| agent.get("name").and_then(Value::as_str).map(str::to_owned))
      .collect()
  };
  let departed = said.as_ref().and_then(|said| said.get("departed")).and_then(Value::as_array);

  HubAnswer::Attached {
    agents: names(agents),
    departed: departed.map(|list| names(list)).unwrap_or_default(),
  }
}
