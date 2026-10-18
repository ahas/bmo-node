//! The node check as a program apart runs it (docs/nodes.md, 「How a node's state is
//! known」): what to check read as JSON from the standing input, what each check met written as JSON to
//! the standing output. The caller writes the rows.

use std::collections::HashMap;
use std::io::Read;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use node_check::hub::HubAnswer;
use node_check::pass::{ask_hubs, check_rows};
use node_check::rows::{Hub, NodeRow};
use node_check::{Machine, NodeCheck};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Parser)]
#[command(name = "node-check", about = "Checks the nodes, reading JSON in and writing JSON out")]
struct Cli {
  #[command(subcommand)]
  step: Step,
}

#[derive(Subcommand)]
enum Step {
  /// Asks every hub handed in as `{"hubs": [{"url", "tokenFile"}]}` which agents are attached, and
  /// answers `{"hubs": [{"url", "answer"}]}`.
  Hubs,
  /// Checks every row handed in as `{"nodes": [...], "hubs": [{"url", "answer"}]}`, a jake node read
  /// off its hub's answer where one is handed, and answers `{"checks": [{"name", "state", "detail",
  /// "at"}]}`.
  Nodes,
}

#[derive(Deserialize)]
struct HubsIn {
  hubs: Vec<Hub>,
}

#[derive(Serialize, Deserialize)]
struct Answered {
  url: String,
  answer: HubAnswer,
}

#[derive(Deserialize)]
struct NodesIn {
  nodes: Vec<NodeRow>,
  #[serde(default)]
  hubs: Vec<Answered>,
}

#[derive(Serialize)]
struct Named {
  name: String,
  #[serde(flatten)]
  check: NodeCheck,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let cli = Cli::parse();
  let mut input = String::new();

  if let Err(error) = std::io::stdin().read_to_string(&mut input) {
    eprintln!("node-check: the standing input could not be read: {error}");
    return ExitCode::FAILURE;
  }

  match answer(cli.step, &input, &Machine::local()).await {
    Ok(said) => {
      println!("{said}");
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("node-check: {error}");
      ExitCode::FAILURE
    }
  }
}

async fn answer(
  step: Step,
  input: &str,
  machine: &Machine,
) -> Result<serde_json::Value, serde_json::Error> {
  match step {
    Step::Hubs => {
      let asked: HubsIn = serde_json::from_str(input)?;
      let hubs: Vec<Answered> = ask_hubs(machine, &asked.hubs)
        .await
        .into_iter()
        .map(|(url, answer)| Answered { url, answer })
        .collect();

      Ok(json!({ "hubs": hubs }))
    }
    Step::Nodes => {
      let asked: NodesIn = serde_json::from_str(input)?;
      let answers: HashMap<String, HubAnswer> =
        asked.hubs.into_iter().map(|one| (one.url, one.answer)).collect();
      let checks: Vec<Named> = check_rows(machine, &asked.nodes, &answers)
        .await
        .into_iter()
        .zip(&asked.nodes)
        .map(|(check, node)| Named {
          name: node.name.clone(),
          check,
        })
        .collect();

      Ok(json!({ "checks": checks }))
    }
  }
}
