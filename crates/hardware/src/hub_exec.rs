//! A script run on a jake node through its hub (docs/hardware.md, 「Which machines」): the hub's exec aimed
//! at the node's own agent, its output handed on as it comes and its ending told apart. The control token
//! is read out of the hub's token file for the one request that needs it and is part of no answer.

use node_check::Machine;
use node_check::hub_exec::{ExecBody, Said, start};
use node_check::rows::Hub;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// How a script run through the hub ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
  /// It ran and ended with this code, and the last thing it said on its standard error.
  Code(i32, String),
  /// The road itself failed, saying why.
  Failed(String),
}

/// A script under way: its output as it comes, and its ending once it has ended.
pub struct Running {
  pub output: mpsc::Receiver<Vec<u8>>,
  pub ended: oneshot::Receiver<Ended>,
}

// How many chunks of a script's output wait for whoever reads them before the script is held.
const WAITING_CHUNKS: usize = 8;

// The most a road's failure says.
const DETAIL_MOST: usize = 200;

fn last_line(text: &str) -> String {
  text
    .lines()
    .map(str::trim)
    .rfind(|line| !line.is_empty())
    .map(|line| line.chars().take(DETAIL_MOST).collect())
    .unwrap_or_default()
}

/// A jake node's road: its hub's exec aimed at its own agent, the token read on the machine given.
#[derive(Clone)]
pub struct HubExec {
  pub hub: Hub,
  pub agent: String,
  /// Whether the agent's host takes a PowerShell command rather than a Linux shell's.
  pub powershell: bool,
  pub machine: Machine,
}

impl HubExec {
  pub fn new(hub: Hub, agent: String, powershell: bool) -> Self {
    Self {
      hub,
      agent,
      powershell,
      machine: Machine::local(),
    }
  }

  /// Starts a script in the agent's login bash.
  pub fn run(&self, script: String) -> Running {
    let (output, waiting) = mpsc::channel(WAITING_CHUNKS);
    let (ending, ended) = oneshot::channel();
    let road = self.clone();

    tokio::spawn(async move {
      let _ = ending.send(road.exec(script, output).await);
    });

    Running {
      output: waiting,
      ended,
    }
  }

  /// The exec a script is handed to the hub as: a Linux shell's command, or on a Windows host a PowerShell
  /// command carrying the script packed, since the agent starts PowerShell on the whole command encoded and
  /// Windows refuses a command line past its limit.
  pub fn body(&self, script: &str) -> ExecBody {
    if self.powershell {
      ExecBody::plain("powershell", crate::windows::packed_script(script))
    } else {
      ExecBody::plain("linux", script)
    }
  }

  /// The same exec as the JSON the hub is handed.
  pub fn request(&self, script: &str) -> Value {
    serde_json::to_value(self.body(script)).expect("an exec's body always renders as JSON")
  }

  async fn exec(&self, script: String, output: mpsc::Sender<Vec<u8>>) -> Ended {
    let mut run = match start(&self.machine, &self.hub, &self.agent, &self.body(&script)).await {
      Ok(run) => run,
      Err(why) => return Ended::Failed(why),
    };
    let mut stderr = String::new();

    loop {
      match run.next().await {
        Ok(Said::Started(_)) => {}
        Ok(Said::Stdout(bytes)) => {
          if output.send(bytes).await.is_err() {
            run.cancel().await;

            return Ended::Failed("nobody listened".into());
          }
        }
        Ok(Said::Stderr(bytes)) => stderr.push_str(&String::from_utf8_lossy(&bytes)),
        Ok(Said::Exit { code, .. }) => {
          let code = code.and_then(|code| i32::try_from(code).ok()).unwrap_or(-1);

          return Ended::Code(code, last_line(&stderr));
        }
        Err(why) => return Ended::Failed(why),
      }
    }
  }
}
