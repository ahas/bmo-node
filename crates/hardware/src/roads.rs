//! A machine's own road for its live run (docs/hardware.md, 「Which machines」): a local shell for this
//! machine, the check's own login for an ssh node — a POSIX shell reading the script from its standard
//! input, or Windows PowerShell handed it as its command — and the hub's exec aimed at the node's own
//! agent for a jake node. Each line the script prints is handed on as it comes, and a run stopped is
//! ended at once.

use std::process::Stdio;

use node_check::Machine;
use node_check::rows::{Hub, SshTarget};
use node_check::ssh::{SshRun, login_args, read_run};
use policy::ssh::SSH;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::hub_exec::{Ended, HubExec};

/// The most a road's failure says.
const DETAIL_MOST: usize = 200;

/// What a machine is read by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
  Local,
  Ssh {
    target: SshTarget,
    powershell: bool,
  },
  /// A hub node, `powershell` where its agent runs on a Windows host and takes a PowerShell command.
  Jake {
    hub: Hub,
    agent: String,
    powershell: bool,
  },
}

impl Source {
  /// Whether the road takes a script in Windows PowerShell rather than a POSIX shell.
  pub fn powershell(&self) -> bool {
    matches!(
      self,
      Self::Ssh {
        powershell: true,
        ..
      } | Self::Jake {
        powershell: true,
        ..
      }
    )
  }

  /// How long one live run lasts where the road itself stops a run: through a jake hub, inside the five
  /// minutes the hub gives an exec. None on a local shell or an ssh login, where a run lasts until its
  /// stream ends.
  pub fn run_seconds(&self) -> Option<i64> {
    match self {
      Self::Jake { .. } => Some(4 * 60),
      _ => None,
    }
  }

  /// What tells one road from another, so a machine whose road changed is read anew.
  pub fn road(&self) -> String {
    format!("{self:?}")
  }
}

/// How a run held open ended: whether its script ended well, and what the road met where it did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamEnd {
  pub ok: bool,
  pub failure: String,
}

fn last_line(text: &str) -> Option<String> {
  text
    .lines()
    .map(str::trim)
    .rfind(|line| !line.is_empty())
    .map(|line| line.chars().take(DETAIL_MOST).collect())
}

/// Lines out of a stream of bytes, each without its carriage return, the last one handed on even
/// where nothing ends it; whether whoever reads them is still there.
struct Lines {
  rest: Vec<u8>,
}

impl Lines {
  async fn feed(&mut self, bytes: &[u8], lines: &mpsc::Sender<String>) -> bool {
    self.rest.extend_from_slice(bytes);

    while let Some(cut) = self.rest.iter().position(|&byte| byte == b'\n') {
      let line: Vec<u8> = self.rest.drain(..=cut).collect();

      if lines.send(text(&line[..line.len() - 1])).await.is_err() {
        return false;
      }
    }

    true
  }

  async fn finish(self, lines: &mpsc::Sender<String>) {
    if !self.rest.is_empty() {
      let _ = lines.send(text(&self.rest)).await;
    }
  }
}

fn text(line: &[u8]) -> String {
  String::from_utf8_lossy(line.strip_suffix(b"\r").unwrap_or(line)).into_owned()
}

async fn read_lines(mut out: impl AsyncRead + Unpin, lines: &mpsc::Sender<String>) {
  let mut buffer = vec![0; 16 * 1024];
  let mut split = Lines { rest: vec![] };

  loop {
    match out.read(&mut buffer).await {
      Ok(0) | Err(_) => break,
      Ok(read) => {
        if !split.feed(&buffer[..read], lines).await {
          return;
        }
      }
    }
  }

  split.finish(lines).await;
}

/// A process held open with its script on its standard input where it takes one, its lines handed on,
/// killed once the run is stopped; its exit code and what it said on its standard error.
async fn stream_process(
  program: &str,
  args: &[String],
  script: Option<&str>,
  lines: &mpsc::Sender<String>,
  stop: &CancellationToken,
) -> Result<(Option<i32>, String), String> {
  let mut child = Command::new(program)
    .args(args)
    .stdin(if script.is_some() { Stdio::piped() } else { Stdio::null() })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .map_err(|why| format!("{program} could not be started: {why}"))?;

  if let (Some(script), Some(mut stdin)) = (script, child.stdin.take()) {
    let script = script.to_owned();

    // A child gone before its script was handed over says so through its exit.
    tokio::spawn(async move {
      let _ = stdin.write_all(script.as_bytes()).await;
    });
  }

  let said = child.stderr.take().map(|mut stderr| {
    tokio::spawn(async move {
      let mut said = String::new();
      let _ = stderr.read_to_string(&mut said).await;
      said
    })
  });
  let out = child.stdout.take();
  let read = async {
    if let Some(out) = out {
      read_lines(out, lines).await;
    }
  };

  tokio::select! {
    () = read => {}
    () = stop.cancelled() => {
      let _ = child.kill().await;
    }
  }

  let code = child.wait().await.ok().and_then(|status| status.code());
  let said = match said {
    Some(task) => task.await.unwrap_or_default(),
    None => String::new(),
  };

  Ok((code, said))
}

/// A script in the road's own language held open on a machine over its own road, each line it prints
/// handed on as it comes, until it ends or `stop` stops it.
pub async fn stream(
  source: &Source,
  script: &str,
  lines: mpsc::Sender<String>,
  stop: CancellationToken,
) -> StreamEnd {
  let ended = |code: Option<i32>, failure: String| StreamEnd {
    ok: code == Some(0),
    failure: if code == Some(0) { String::new() } else { failure },
  };

  match source {
    Source::Local => match stream_process("sh", &["-s".into()], Some(script), &lines, &stop).await {
      Ok((code, said)) => ended(
        code,
        last_line(&said).unwrap_or_else(|| format!("the script ended with {}", shown(code))),
      ),
      Err(why) => StreamEnd {
        ok: false,
        failure: why,
      },
    },
    Source::Ssh { target, powershell } => {
      let machine = Machine::local();
      let (command, stdin) = if *powershell {
        (super::windows::powershell_command(script), None)
      } else {
        ("sh -s".to_owned(), Some(script))
      };
      let args = login_args(&machine, &SSH.batch, target, Some(&command));

      match stream_process(&machine.ssh.to_string_lossy(), &args, stdin, &lines, &stop).await {
        Ok((code, said)) => {
          let met = read_run(&SshRun {
            code,
            stderr: said.clone(),
          });

          ended(
            code,
            met
              .detail
              .or_else(|| last_line(&said))
              .unwrap_or_else(|| format!("ssh ended with {}", shown(code))),
          )
        }
        Err(why) => StreamEnd {
          ok: false,
          failure: why,
        },
      }
    }
    Source::Jake {
      hub,
      agent,
      powershell,
    } => stream_through(&HubExec::new(hub.clone(), agent.clone(), *powershell), script, lines, stop).await,
  }
}

/// A script held open on a jake node through its hub, each line it prints handed on as it comes, until it
/// ends or `stop` stops it.
pub async fn stream_through(
  road: &HubExec,
  script: &str,
  lines: mpsc::Sender<String>,
  stop: CancellationToken,
) -> StreamEnd {
  let mut running = road.run(script.to_owned());
  let mut split = Lines { rest: vec![] };

  loop {
    tokio::select! {
      chunk = running.output.recv() => match chunk {
        Some(chunk) => {
          if !split.feed(&chunk, &lines).await {
            break;
          }
        }
        None => break,
      },
      // Whoever stops listening stops the exec: the road cancels it on the hub at once.
      () = stop.cancelled() => break,
    }
  }

  if stop.is_cancelled() {
    return StreamEnd {
      ok: false,
      failure: "the run was stopped".into(),
    };
  }

  split.finish(&lines).await;

  match running.ended.await {
    Ok(Ended::Code(0, _)) => StreamEnd {
      ok: true,
      failure: String::new(),
    },
    Ok(Ended::Code(code, said)) => StreamEnd {
      ok: false,
      failure: last_line(&said).unwrap_or_else(|| format!("the script ended with {code}")),
    },
    Ok(Ended::Failed(why)) => StreamEnd {
      ok: false,
      failure: why.chars().take(DETAIL_MOST).collect(),
    },
    Err(_) => StreamEnd {
      ok: false,
      failure: "the hub's stream closed without the command's end".into(),
    },
  }
}

fn shown(code: Option<i32>) -> String {
  code.map_or_else(|| "no code".to_owned(), |code| code.to_string())
}
