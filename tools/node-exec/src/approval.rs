//! The owner's approval of a command that changes a machine (docs/tools.md, 「A command that changes a machine」):
//! an install, a stop of a process or a service, a restart or a shutdown runs only once the owner chose to run it,
//! asked as a question of the job service. The tool asks, refuses the call naming the question, and runs the same
//! command once the call names that question answered by a person's choice to run it.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

/// The choice that runs the command, and the one that does not.
pub const RUN: &str = "Run it";
pub const SKIP: &str = "Do not run it";

// The words a command that changes a machine is read by: the tool runs PowerShell, WSL's bash and a Linux host's
// shell alike, and no reading of a command line serves all three, so the command's text is searched for the words
// rather than read as one shell's line. The words are each a program or a verb standing as a word of its own:
// a package manager's install or removal, a stop of a process or a service, a restart or a shutdown. Matched as
// whole words in the command's text, whatever shell it is in.
const CHANGES: [&str; 26] = [
  "apt",
  "apt-get",
  "dnf",
  "yum",
  "pacman",
  "snap",
  "brew",
  "winget",
  "choco",
  "scoop",
  "msiexec",
  "dpkg",
  "kill",
  "pkill",
  "killall",
  "taskkill",
  "Stop-Process",
  "Stop-Service",
  "Restart-Service",
  "Restart-Computer",
  "Stop-Computer",
  "shutdown",
  "reboot",
  "poweroff",
  "systemctl",
  "sc.exe",
];

/// The word that makes a command one that changes a machine, none for any other.
pub fn changes_machine(command: &str) -> Option<&'static str> {
  let words: Vec<&str> = command
    .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')' | '`' | '"' | '\''))
    .filter(|word| !word.is_empty())
    .collect();

  CHANGES
    .into_iter()
    .find(|change| words.iter().any(|word| word.eq_ignore_ascii_case(change)))
}

/// The question's words for a command on a node, which an approval must still read as it did.
pub fn asked_text(node: &str, command: &str) -> String {
  format!("Run this command on the node {node}?\n\n{command}")
}

/// The job service as the tool asks it: its address and token as the data root's `env/agency.env` names them.
pub struct Questions {
  base: String,
  token: String,
  http: reqwest::Client,
}

impl Questions {
  /// The job service at `base`, asked with `token`.
  pub fn new(base: &str, token: &str) -> Self {
    Self {
      base: base.trim_end_matches('/').to_owned(),
      token: token.to_owned(),
      http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default(),
    }
  }

  /// The job service agency's configuration names, a variable of this process's own environment standing first.
  pub fn read(data_root: &Path) -> Result<Self, String> {
    let file = data_root.join("env/agency.env");
    let mut pairs: Vec<(String, String)> = match dotenvy::from_path_iter(&file) {
      Ok(pairs) => pairs.filter_map(Result::ok).collect(),
      Err(_) => Vec::new(),
    };

    pairs.extend(std::env::vars());

    let named =
      |key: &str| pairs.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value.clone());
    let base = named("BMO_JOB_URL").ok_or_else(|| format!("{} names no job service", file.display()))?;
    let token =
      named("BMO_JOB_TOKEN").ok_or_else(|| format!("{} names no job service token", file.display()))?;

    Ok(Self {
      base: base.trim_end_matches('/').to_owned(),
      token,
      http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default(),
    })
  }

  /// The owner asked whether to run the command, the question's number answered.
  pub async fn ask(&self, asker: &str, node: &str, command: &str) -> Result<i64, String> {
    let body = json!({
      "askerName": asker,
      "text": asked_text(node, command),
      "choices": [RUN, SKIP],
      "addressee": "person",
      "recommendation": SKIP,
    });
    let answer = self
      .http
      .post(format!("{}/questions", self.base))
      .bearer_auth(&self.token)
      .json(&body)
      .send()
      .await
      .map_err(|why| format!("the job service did not answer: {why}"))?;
    let status = answer.status().as_u16();
    let read: Value = answer.json().await.unwrap_or(Value::Null);

    read["id"]
      .as_i64()
      .filter(|_| status == 201)
      .ok_or_else(|| format!("the job service answered {status}: {read}"))
  }

  /// Whether question `id` approves the command on the node: it asks of that very command, and a person settled it
  /// by choosing to run it. A question still open, answered otherwise, settled by its recommendation or by a
  /// supervisor approves nothing.
  pub async fn approves(&self, id: i64, node: &str, command: &str) -> Result<bool, String> {
    let question = self.get(&format!("/questions/{id}")).await?;

    if question["text"].as_str() != Some(asked_text(node, command).as_str())
      || question["state"] != "answered"
    {
      return Ok(false);
    }

    let replies = self.get(&format!("/questions/{id}/replies")).await?;

    Ok(replies["items"].as_array().into_iter().flatten().any(|reply| {
      reply["settles"] == true && reply["source"] == "person" && reply["choice"].as_str() == Some(RUN)
    }))
  }

  async fn get(&self, path: &str) -> Result<Value, String> {
    let answer = self
      .http
      .get(format!("{}{path}", self.base))
      .bearer_auth(&self.token)
      .send()
      .await
      .map_err(|why| format!("the job service did not answer: {why}"))?;
    let status = answer.status().as_u16();

    if status != 200 {
      return Err(format!("the job service answered {status} to {path}"));
    }

    answer
      .json()
      .await
      .map_err(|why| format!("the job service's answer does not read: {why}"))
  }
}
