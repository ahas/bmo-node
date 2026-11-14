//! The hosted tool `node-exec` (docs/tools.md): one entry, `bmo/node_exec`, which runs a command on a hub
//! node through its hub for a caller whose account owns the node or that holds the administrator's grant,
//! and answers the command's exit code and output. One call is handed on standard input and answered on
//! standard output (bmo-pocket's `docs/api.md`, 「The call protocol」).

pub mod caller;

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmo_node_client::NodeClient;
use bmo_node_client::nodes::NodeKind;
use bmo_node_client::runs::{Run, RunShell};
use node_check::Machine;
use node_check::hub_exec::{Broke, ExecBody, run};
use node_check::rows::Hub;
use node_server::config::Config;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::caller::{Agency, Session, account, judge, node_of};

/// The entry this executable answers, as its manifest declares it.
pub const ENTRY: &str = "bmo/node_exec";

/// One call handed by the tool host.
#[derive(Debug, Deserialize)]
pub struct Handed {
  #[serde(default)]
  pub entry: String,
  #[serde(default)]
  pub arguments: Value,
  #[serde(default)]
  pub session: Session,
}

/// The entry's arguments.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
  #[serde(default)]
  pub node: Option<String>,
  #[serde(default)]
  pub account: Option<Value>,
  pub command: String,
  #[serde(default)]
  pub shell: Option<RunShell>,
  #[serde(default)]
  pub cwd: Option<String>,
  #[serde(default)]
  pub env: BTreeMap<String, String>,
  #[serde(default)]
  pub distro: Option<String>,
}

impl Ask {
  /// The node the call is aimed at: the one it names, or the one agency's records name for the account it
  /// names; the reason where it names both or neither, or its account has no node.
  async fn node(&self, agency: Result<&Agency, &String>) -> Result<String, String> {
    match (&self.node, &self.account) {
      (Some(node), None) => Ok(node.clone()),
      (None, Some(named)) => {
        let id = account(Some(named)).ok_or_else(|| format!("the account {named} is no account's id"))?;

        node_of(&id, agency).await
      }
      _ => Err("Name the node by node or by account, one of the two.".to_owned()),
    }
  }
}

/// The host's tool result: a person's reading first, and the command's end for a program beside it.
#[derive(Debug, Serialize)]
pub struct Answer {
  pub content: Vec<Value>,
  #[serde(rename = "isError")]
  pub is_error: bool,
  #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
  pub structured: Option<Value>,
}

impl Answer {
  fn ran(ran: &Run) -> Self {
    let end = match (&ran.exit_code, &ran.signal) {
      (Some(code), _) => format!("exit code {code}"),
      (None, Some(signal)) => format!("ended by {signal}"),
      (None, None) => "no exit code".to_owned(),
    };
    let mut text = format!("{} ({}): {end}", ran.node, ran.shell);

    for (name, said, cut) in
      [("stdout", &ran.stdout, ran.stdout_cut), ("stderr", &ran.stderr, ran.stderr_cut)]
    {
      if !said.is_empty() {
        let cut = if cut { ", its last 64 KiB" } else { "" };

        text.push_str(&format!("\n--- {name}{cut}\n{said}"));
      }
    }

    Self {
      content: vec![json!({ "type": "text", "text": text })],
      is_error: false,
      structured: serde_json::to_value(ran).ok(),
    }
  }

  fn broke(node: &str, broke: &Broke) -> Self {
    let mut text = format!("The command on {node} has no known end: {}.", broke.why);

    for (name, said) in [("stdout", &broke.stdout), ("stderr", &broke.stderr)] {
      if !said.is_empty() {
        text.push_str(&format!("\n--- {name} before that\n{said}"));
      }
    }

    Self::refusal(text)
  }

  /// A refusal saying why in English.
  pub fn refusal(why: impl Into<String>) -> Self {
    Self {
      content: vec![json!({ "type": "text", "text": why.into() })],
      is_error: true,
      structured: None,
    }
  }

  /// The answer's text, as a person reads it.
  pub fn text(&self) -> String {
    self
      .content
      .iter()
      .filter_map(|one| one.get("text").and_then(Value::as_str))
      .collect::<Vec<_>>()
      .join("\n")
  }
}

/// What a call reaches: the node service, agency, and this machine's hub requests and home. Each is read
/// where it is first needed, and a part that could not be read is the reason a call needing it is refused.
pub struct Ground {
  pub nodes: Result<NodeClient, String>,
  pub agency: Result<Agency, String>,
  pub machine: Machine,
}

impl Ground {
  /// The node service and agency as the data root's configuration names them.
  pub fn read(data_root: &Path) -> Self {
    let nodes = Config::read(&data_root.join("env/node.toml"))
      .and_then(|config| NodeClient::new(&config.local_address(), config.token));

    Self {
      nodes,
      agency: Agency::read(data_root),
      machine: Machine::local(),
    }
  }

  /// The ground of a call whose environment names no data root.
  fn unrooted() -> Self {
    let why = "the call's environment names no BMO_DATA_DIR".to_owned();

    Self {
      nodes: Err(why.clone()),
      agency: Err(why),
      machine: Machine::local(),
    }
  }
}

/// The call this process was handed, answered on standard output.
pub async fn run_call() -> ExitCode {
  let mut raw = String::new();

  if let Err(why) = std::io::stdin().read_to_string(&mut raw) {
    eprintln!("the call could not be read: {why}");
    return ExitCode::FAILURE;
  }

  let answer = match serde_json::from_str::<Handed>(&raw) {
    Ok(handed) => {
      let ground = match std::env::var_os("BMO_DATA_DIR") {
        Some(root) if !root.is_empty() => Ground::read(&PathBuf::from(root)),
        _ => Ground::unrooted(),
      };

      answer(&handed, &ground).await
    }
    Err(why) => Answer::refusal(format!("the call does not read: {why}")),
  };

  write(&answer)
}

fn write(answer: &Answer) -> ExitCode {
  let text = match serde_json::to_string(answer) {
    Ok(text) => text,
    Err(why) => {
      eprintln!("the answer could not be written: {why}");
      return ExitCode::FAILURE;
    }
  };

  match std::io::stdout().lock().write_all(text.as_bytes()) {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("the answer could not be written: {why}");
      ExitCode::FAILURE
    }
  }
}

/// The entry asked for, answered over `ground`: the node an account names read first, then the caller
/// judged, then the node read, then the command run on it.
pub async fn answer(handed: &Handed, ground: &Ground) -> Answer {
  if handed.entry != ENTRY {
    return Answer::refusal(format!("this executable holds no entry {}", handed.entry));
  }

  let ask = match serde_json::from_value::<Ask>(handed.arguments.clone()) {
    Ok(ask) => ask,
    Err(why) => return Answer::refusal(format!("the arguments are not the shape this tool takes: {why}")),
  };

  let node = match ask.node(ground.agency.as_ref()).await {
    Ok(node) => node,
    Err(why) => return Answer::refusal(why),
  };

  if let Err(why) = judge(&handed.session, &node, ground.agency.as_ref()).await {
    return Answer::refusal(why);
  }

  let nodes = match &ground.nodes {
    Ok(nodes) => nodes,
    Err(why) => return Answer::refusal(format!("the node service cannot be reached: {why}")),
  };
  let target = match nodes.target(&node).await {
    Ok(target) => target,
    Err(why) => return Answer::refusal(format!("the node {node} could not be read: {why}")),
  };

  if target.kind != NodeKind::Hub {
    return Answer::refusal(format!(
      "The node {node} is an ssh node, which this tool does not reach: log in with its target's sshArgs."
    ));
  }

  let (Some(url), Some(token_file), Some(agent)) =
    (target.hub_address, target.hub_token_file, target.agent_name)
  else {
    return Answer::refusal(format!("the node {node} names no hub and agent to reach it by"));
  };
  let body = ExecBody {
    shell: ask.shell.unwrap_or_else(|| RunShell::of_host(target.shell)).name().to_owned(),
    command: ask.command,
    cwd: ask.cwd,
    env: ask.env,
    distro: ask.distro,
  };
  let hub = Hub { url, token_file };

  match run(&ground.machine, &hub, &agent, &node, &body).await {
    Ok(ran) => Answer::ran(&ran),
    Err(broke) => Answer::broke(&node, &broke),
  }
}
