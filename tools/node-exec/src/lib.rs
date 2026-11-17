//! The hosted tool `node-exec` (docs/tools.md): one entry, `bmo/node_exec`, which runs a command on a hub
//! node through its hub for a caller whose account owns the node or that holds the administrator's grant,
//! and answers the command's exit code and output. One call is handed on standard input and answered on
//! standard output (bmo-pocket's `docs/api.md`, 「The call protocol」).

pub mod caller;
pub mod script;
pub mod secrets;

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

use crate::caller::{Agency, Caller, Session, account, judge, node_of};
use crate::script::{Script, folder_id};
use crate::secrets::{Handing, Named, read};

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
  #[serde(default)]
  pub secrets: Vec<Named>,
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

  /// The answer with a sentence of its own after what it says.
  fn noting(mut self, note: String) -> Self {
    self.content.push(json!({ "type": "text", "text": note }));
    self
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

/// What a call reaches: the node service, agency, the data root's `env/` the secrets stand in, and this
/// machine's hub requests and home. Each is read where it is first needed, and a part that could not be read is
/// the reason a call needing it is refused.
pub struct Ground {
  pub nodes: Result<NodeClient, String>,
  pub agency: Result<Agency, String>,
  pub secrets: Result<PathBuf, String>,
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
      secrets: Ok(data_root.join("env")),
      machine: Machine::local(),
    }
  }

  /// The ground of a call whose environment names no data root.
  fn unrooted() -> Self {
    let why = "the call's environment names no BMO_DATA_DIR".to_owned();

    Self {
      nodes: Err(why.clone()),
      agency: Err(why.clone()),
      secrets: Err(why),
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

/// The secrets a call names, read from the space its caller reads and laid beside its own variables; none
/// where it names none.
fn handing(ask: &Ask, caller: &Caller, ground: &Ground) -> Result<Handing, String> {
  if ask.secrets.is_empty() {
    return Ok(Handing::default());
  }

  let env = ground.secrets.as_ref().map_err(|why| format!("no secret can be read: {why}"))?;
  let secrets = read(&ask.secrets, &caller.space(env)?)?;

  Handing::of(&secrets, &ask.env)
}

/// The entry asked for, answered over `ground`: the node an account names read first, then the caller
/// judged, then the secrets it names read, then the node read, then the command run on it, and every value
/// handed masked in what the answer says.
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

  let caller = match judge(&handed.session, &node, ground.agency.as_ref()).await {
    Ok(caller) => caller,
    Err(why) => return Answer::refusal(why),
  };
  let handing = match handing(&ask, &caller, ground) {
    Ok(handing) => handing,
    Err(why) => return Answer::refusal(why),
  };

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
  let shell = ask.shell.unwrap_or_else(|| RunShell::of_host(target.shell));
  let script = Script::of(shell, target.shell);
  let folder = if handing.files.is_empty() {
    None
  } else {
    match folder_id() {
      Ok(id) => Some(id),
      Err(why) => return Answer::refusal(why),
    }
  };
  let mut env = ask.env.clone();

  env.extend(handing.env.clone());

  let body = ExecBody {
    shell: shell.name().to_owned(),
    command: match &folder {
      Some(id) => script.wrap(id, &handing.files, &ask.command),
      None => ask.command.clone(),
    },
    cwd: ask.cwd.clone(),
    env,
    distro: ask.distro.clone(),
  };
  let hub = Hub { url, token_file };
  let answered = match run(&ground.machine, &hub, &agent, &node, &body).await {
    Ok(ran) => Answer::ran(&masked_run(ran, &handing)),
    Err(broke) => Answer::broke(&node, &masked_broke(broke, &handing)),
  };
  let Some(id) = folder else {
    return answered;
  };

  // A kill skips the handing script's own removal, so the folder is removed again whatever the end was.
  let removal = ExecBody {
    shell: body.shell,
    command: script.removal(&id),
    cwd: None,
    env: ask.env,
    distro: ask.distro,
  };

  match unconfirmed(&ground.machine, &hub, &agent, &node, &removal).await {
    Some(why) => answered.noting(format!(
      "Whether the secrets' folder on {node} was removed could not be confirmed: {}; the next call handing a \
       file there removes it once a day old.",
      handing.masked(&why)
    )),
    None => answered,
  }
}

/// Why the removal of a call's folder is not known to have happened; none where it ended with code 0.
async fn unconfirmed(
  machine: &Machine,
  hub: &Hub,
  agent: &str,
  node: &str,
  removal: &ExecBody,
) -> Option<String> {
  match run(machine, hub, agent, node, removal).await {
    Ok(ran) if ran.exit_code == Some(0) => None,
    Ok(ran) => Some(match ran.exit_code {
      Some(code) => format!("its removal ended with exit code {code}"),
      None => "its removal ended with no exit code".to_owned(),
    }),
    Err(broke) => Some(broke.why),
  }
}

fn masked_run(ran: Run, handing: &Handing) -> Run {
  Run {
    stdout: handing.masked(&ran.stdout),
    stderr: handing.masked(&ran.stderr),
    ..ran
  }
}

fn masked_broke(broke: Broke, handing: &Handing) -> Broke {
  Broke {
    why: handing.masked(&broke.why),
    stdout: handing.masked(&broke.stdout),
    stderr: handing.masked(&broke.stderr),
  }
}
