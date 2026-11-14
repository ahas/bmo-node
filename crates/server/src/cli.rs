//! The `bmo-node node` and `bmo-node hardware-setup` commands (docs/nodes.md, 「Registering and changing a
//! node」; docs/hardware.md, 「The Windows side」): the node list read and changed against the store, a node
//! checked now, a node's target printed for a tool, and the Windows of each node made readable in full.
//! No line holds a credential, only where one is kept.

use clap::Parser;
use hardware::roads::{Source, stream};
use hardware::source_of;
use hardware::windows::encoded_command;
use node_check::Machine;
use node_check::pass::check_nodes;
use policy::nodes::NodeKind;
use policy::windows_sensors::WINDOWS_SENSORS;
use store::Store;
use store::nodes::{DEFAULT_LOGIN_SHELL, Hub, LOGIN_SHELLS, Node, NodeChanges, SshLogin};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::views;

/// The nouns `bmo` dispatches to this command, each marked `home`: both act on the node list the service
/// keeps, so the command runs where the service stands.
pub const NOUNS: [&str; 2] = ["node\thome", "hardware-setup\thome"];

/// How the command is run, said under each refusal of its arguments.
pub const NODES_USAGE: &str = "usage: bmo-node --config <file> node
       bmo-node --config <file> node --add <name> --ssh <user@host> [--port <n>] [--key <path>] [--shell posix|powershell] [--label <label>] [--role <service>]
       bmo-node --config <file> node --set <name> [--label <label>] [--role <service>|none] [--callers <caller>[+<caller>]|none] [--ssh <user@host>] [--port <n>] [--key <path>|none] [--shell posix|powershell]
       bmo-node --config <file> node --remove <name>
       bmo-node --config <file> node --add-hub <control url> --token-file <path>
       bmo-node --config <file> node --remove-hub <control url>
       bmo-node --config <file> node --check [<name>]
       bmo-node --config <file> node --target <name>";

/// The command's arguments, one of its verb flags at a time, and none to list.
#[derive(Parser, Debug, Default)]
pub struct NodesArgs {
  #[arg(long)]
  pub add: Option<String>,
  #[arg(long)]
  pub set: Option<String>,
  #[arg(long)]
  pub remove: Option<String>,
  #[arg(long = "add-hub")]
  pub add_hub: Option<String>,
  #[arg(long = "remove-hub")]
  pub remove_hub: Option<String>,
  /// One node by its name, or every node.
  #[arg(long, num_args = 0..=1, default_missing_value = "")]
  pub check: Option<String>,
  #[arg(long)]
  pub target: Option<String>,
  #[arg(long)]
  pub ssh: Option<String>,
  #[arg(long)]
  pub port: Option<String>,
  #[arg(long)]
  pub key: Option<String>,
  #[arg(long)]
  pub shell: Option<String>,
  #[arg(long)]
  pub label: Option<String>,
  #[arg(long)]
  pub role: Option<String>,
  /// The callers the node lets make a run on it, joined by `+`, or `none`.
  #[arg(long)]
  pub callers: Option<String>,
  #[arg(long = "token-file")]
  pub token_file: Option<String>,
}

/// What the command is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeCommand {
  List,
  Add {
    name: String,
    label: Option<String>,
    role: Option<String>,
    login: SshLogin,
  },
  Set {
    name: String,
    changes: NodeChanges,
    /// The callers set whole, or cleared by none; left as they are where not given.
    callers: Option<Option<String>>,
  },
  Remove(String),
  AddHub {
    url: String,
    token_file: String,
  },
  RemoveHub(String),
  Check(Option<String>),
  Target(String),
}

fn port(text: &str) -> Result<i64, String> {
  match text.parse::<i64>() {
    Ok(port) if text.chars().all(|one| one.is_ascii_digit()) => Ok(port),
    _ => Err(format!("a port is a whole number, not {text}")),
  }
}

fn shell(text: &str) -> Result<String, String> {
  if LOGIN_SHELLS.contains(&text) {
    Ok(text.to_owned())
  } else {
    Err(format!("--shell is one of {}, not {text}", LOGIN_SHELLS.join(", ")))
  }
}

fn none_or(text: &str) -> Option<String> {
  (text != "none").then(|| text.to_owned())
}

/// `user@host` as a person writes it, split into its two halves.
pub fn ssh_login(text: &str) -> Result<(String, String), String> {
  match text.rfind('@') {
    Some(at) if at > 0 && at < text.len() - 1 => Ok((text[..at].to_owned(), text[at + 1..].to_owned())),
    _ => Err(format!("an ssh target is written user@host, not \"{text}\"")),
  }
}

impl NodesArgs {
  // The flags given, by their names as a person writes them, in the order a person reads them.
  fn given(&self) -> Vec<&'static str> {
    [
      ("--add", self.add.is_some()),
      ("--set", self.set.is_some()),
      ("--remove", self.remove.is_some()),
      ("--add-hub", self.add_hub.is_some()),
      ("--remove-hub", self.remove_hub.is_some()),
      ("--check", self.check.is_some()),
      ("--target", self.target.is_some()),
      ("--ssh", self.ssh.is_some()),
      ("--port", self.port.is_some()),
      ("--key", self.key.is_some()),
      ("--shell", self.shell.is_some()),
      ("--label", self.label.is_some()),
      ("--role", self.role.is_some()),
      ("--callers", self.callers.is_some()),
      ("--token-file", self.token_file.is_some()),
    ]
    .into_iter()
    .filter_map(|(flag, given)| given.then_some(flag))
    .collect()
  }

  /// What the arguments ask, or why they make no sense, refused before anything is written.
  pub fn command(self) -> Result<NodeCommand, String> {
    const VERBS: [&str; 7] =
      ["--add", "--set", "--remove", "--add-hub", "--remove-hub", "--check", "--target"];
    let given = self.given();
    let verbs: Vec<&str> = given.iter().copied().filter(|flag| VERBS.contains(flag)).collect();

    if verbs.len() > 1 {
      return Err(format!("one of {} at a time", VERBS.join(", ")));
    }

    let Some(verb) = verbs.first().copied() else {
      if !given.is_empty() {
        return Err(format!("{} says nothing without {}", given.join(", "), VERBS.join(", ")));
      }

      return Ok(NodeCommand::List);
    };

    match verb {
      "--check" => return Ok(NodeCommand::Check(self.check.filter(|name| !name.is_empty()))),
      "--remove" => return Ok(NodeCommand::Remove(self.remove.unwrap_or_default())),
      "--target" if given.len() > 1 => {
        return Err("--target takes a node's name and nothing else".to_owned());
      }
      "--target" => return Ok(NodeCommand::Target(self.target.unwrap_or_default())),
      "--remove-hub" => return Ok(NodeCommand::RemoveHub(self.remove_hub.unwrap_or_default())),
      "--add-hub" => {
        let others = given.iter().any(|flag| *flag != "--add-hub" && *flag != "--token-file");

        return match self.token_file {
          Some(token_file) if !others => Ok(NodeCommand::AddHub {
            url: self.add_hub.unwrap_or_default(),
            token_file,
          }),
          _ => Err("--add-hub takes --token-file <path> and nothing else".to_owned()),
        };
      }
      _ => {}
    }

    if self.token_file.is_some() {
      return Err(
        "--token-file belongs to --add-hub; a hub node is added when its agent attaches".to_owned(),
      );
    }

    if verb == "--add" {
      if self.callers.is_some() {
        return Err("--callers belongs to --set: a node lets no caller until the owner sets it".to_owned());
      }

      let Some(ssh) = self.ssh else {
        return Err(
          "--add takes --ssh <user@host>; a hub node is added when its agent attaches to a listed hub"
            .to_owned(),
        );
      };
      let (user, host) = ssh_login(&ssh)?;

      return Ok(NodeCommand::Add {
        name: self.add.unwrap_or_default(),
        label: self.label,
        role: self.role.as_deref().and_then(none_or),
        login: SshLogin {
          host,
          port: port(self.port.as_deref().unwrap_or("22"))?,
          user,
          key_path: self.key.as_deref().and_then(none_or),
          shell: shell(self.shell.as_deref().unwrap_or(DEFAULT_LOGIN_SHELL))?,
        },
      });
    }

    let mut changes = NodeChanges {
      label: self.label,
      role: self.role.as_deref().map(none_or),
      ..NodeChanges::default()
    };

    if let Some(ssh) = &self.ssh {
      let (user, host) = ssh_login(ssh)?;

      changes.user = Some(user);
      changes.host = Some(host);
    }

    changes.port = self.port.as_deref().map(port).transpose()?;
    changes.key_path = self.key.as_deref().map(none_or);
    changes.shell = self.shell.as_deref().map(shell).transpose()?;

    let callers = self.callers.as_deref().map(none_or);

    if changes == NodeChanges::default() && callers.is_none() {
      return Err("--set changes nothing without a field to change".to_owned());
    }

    Ok(NodeCommand::Set {
      name: self.set.unwrap_or_default(),
      changes,
      callers,
    })
  }
}

/// A moment as the list writes it, in UTC; `-` for none.
pub fn clock(seconds: Option<i64>) -> String {
  seconds
    .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
    .map_or_else(|| "-".to_owned(), |at| at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
}

/// The hubs as the command prints them, one line each.
pub fn format_hub_list(hubs: &[Hub]) -> String {
  if hubs.is_empty() {
    return "(no hubs)".to_owned();
  }

  hubs
    .iter()
    .map(|hub| {
      let label = hub.label.as_deref().map(|label| format!(" ({label})")).unwrap_or_default();

      format!("hub {} {}{label}, token file {}", hub.id, hub.url, hub.token_file)
    })
    .collect::<Vec<_>>()
    .join("\n")
}

/// The list as the command prints it: one line a node, its state under it, and its detail under that where
/// there is one.
pub fn format_node_list(nodes: &[Node]) -> String {
  if nodes.is_empty() {
    return "(no nodes)".to_owned();
  }

  nodes
    .iter()
    .map(|node| {
      let credential = match node.kind {
        NodeKind::Ssh => format!("key {}", node.key_path.as_deref().unwrap_or("(ssh config)")),
        NodeKind::Hub => {
          format!("token file {}", node.token_file.as_deref().unwrap_or("(hub not listed)"))
        }
      };
      let reserved = node.role.as_deref().map(|role| format!(", {role} only")).unwrap_or_default();
      let callers = node
        .callers
        .as_deref()
        .map(|callers| format!(", runs by {callers}"))
        .unwrap_or_default();
      let head = format!(
        "{} [{}] {} — {}, {credential}{reserved}{callers}",
        node.name,
        node.kind.name(),
        node.label,
        node.address()
      );
      let state = format!(
        "    {} · checked {} · last connected {}",
        node.state.name(),
        clock(node.checked_at),
        clock(node.connected_at)
      );

      match &node.detail {
        None => format!("{head}\n{state}"),
        Some(detail) => format!("{head}\n{state}\n    {detail}"),
      }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn store_refusal(error: store::StoreError) -> String {
  match error {
    store::StoreError::Refused(refusal) => refusal.message,
    other => other.to_string(),
  }
}

/// One run of `bmo-node node`: the lines it prints, or the refusal it ends with.
pub async fn nodes(store: &Store, machine: &Machine, args: NodesArgs) -> Result<Vec<String>, String> {
  let command = args.command().map_err(|why| format!("{why}\n{NODES_USAGE}"))?;

  Ok(match command {
    NodeCommand::List => vec![format!(
      "{}\n\n{}",
      format_node_list(&store.nodes().map_err(store_refusal)?),
      format_hub_list(&store.hubs().map_err(store_refusal)?)
    )],
    NodeCommand::AddHub { url, token_file } => {
      vec![format_hub_list(&[store.add_hub(&url, &token_file, None).map_err(store_refusal)?])]
    }
    NodeCommand::RemoveHub(url) => {
      let hub = store.hub_at(&url).map_err(store_refusal)?.ok_or_else(|| format!("no hub at {url}"))?;

      if store.remove_hub(hub.id).map_err(store_refusal)?.is_none() {
        return Err(format!("no hub at {url}"));
      }

      vec![format!("removed the hub {} and every node reached through it", hub.url)]
    }
    NodeCommand::Add {
      name,
      label,
      role,
      login,
    } => {
      vec![format_node_list(&[store
        .add_node(&name, label.as_deref(), role, login)
        .map_err(store_refusal)?])]
    }
    NodeCommand::Set {
      name,
      changes,
      callers,
    } => {
      let mut node = if changes == NodeChanges::default() {
        store
          .node(&name)
          .map_err(store_refusal)?
          .ok_or_else(|| format!("no node named {name}"))?
      } else {
        store.update_node(&name, changes).map_err(store_refusal)?
      };

      if let Some(callers) = callers {
        node = store.set_callers(&name, callers).map_err(store_refusal)?;
      }

      vec![format_node_list(&[node])]
    }
    NodeCommand::Remove(name) => {
      if !store.remove_node(&name).map_err(store_refusal)? {
        return Err(format!("no node named {name}"));
      }

      vec![format!("removed {name}")]
    }
    NodeCommand::Target(name) => {
      let node = store
        .node(&name)
        .map_err(store_refusal)?
        .ok_or_else(|| format!("no node named {name}"))?;

      vec![
        serde_json::to_string(
          &views::target(&node, machine)
            .map_err(|_| format!("{name} lacks a field its connection type needs"))?,
        )
        .map_err(|why| why.to_string())?,
      ]
    }
    NodeCommand::Check(name) => {
      if let Some(name) = &name
        && !store.node_exists(name).map_err(store_refusal)?
      {
        return Err(format!("no node named {name}"));
      }

      check_nodes(store, machine, name.as_deref()).await.map_err(store_refusal)?;

      let checked: Vec<Node> = store
        .nodes()
        .map_err(store_refusal)?
        .into_iter()
        .filter(|node| name.as_ref().is_none_or(|name| node.name == *name))
        .collect();

      vec![format_node_list(&checked)]
    }
  })
}

const WINDOWS_POWERSHELL: &str = "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";

// What the setup prints where there is no Windows under the machine to set up.
const NOT_WINDOWS: &str = "@@not-windows";

/// What makes a Windows machine readable in full: the sensor library unpacked where the sensor script looks
/// for it, and the driver it reads the CPU and the board through. Run again, it changes nothing that is
/// already in place.
pub fn windows_setup_script() -> String {
  let library = &WINDOWS_SENSORS.library_dir;
  let release = &WINDOWS_SENSORS.release;
  let driver = &WINDOWS_SENSORS.driver_package;

  format!(
    r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$dir = Join-Path $env:ProgramData '{library}'
$marker = Join-Path $dir 'version.txt'
if ((Test-Path $marker) -and ((Get-Content $marker) -eq '{release}')) {{ "library {release} already in place" }} else {{
  $zip = Join-Path $env:TEMP 'bmo-lhm.zip'
  Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/{release}/LibreHardwareMonitor.zip' -OutFile $zip
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Expand-Archive -Path $zip -DestinationPath $dir -Force
  Remove-Item $zip
  Get-ChildItem $dir -Recurse -File | Unblock-File
  Set-Content -Path $marker -Value '{release}'
  "library {release} unpacked into $dir"
}}
if (Get-Service PawnIO -ErrorAction SilentlyContinue) {{ "driver PawnIO already installed" }} else {{
  winget install --id {driver} -e --silent --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
  if (Get-Service PawnIO -ErrorAction SilentlyContinue) {{ "driver PawnIO installed" }} else {{ throw 'the PawnIO driver did not install' }}
}}
"#
  )
}

/// The shell line that runs a PowerShell script on the Windows under WSL with its errors kept and its
/// carriage returns taken out, saying outright where there is no WSL or no PowerShell to hand it to.
pub fn windows_shell_line(script: &str) -> String {
  format!(
    "if grep -qi microsoft /proc/version 2>/dev/null && [ -x {WINDOWS_POWERSHELL} ]; then {WINDOWS_POWERSHELL} -NoProfile -NonInteractive -EncodedCommand {} </dev/null 2>&1 | tr -d '\\r'; else echo {NOT_WINDOWS}; fi",
    encoded_command(script)
  )
}

/// What one machine's setup came to.
#[derive(Debug, PartialEq, Eq)]
pub enum WindowsSetup {
  Done(Vec<String>),
  NotWindows,
  Failed { detail: String, lines: Vec<String> },
}

/// One setup's answer read: placed where it reported both its library and its driver and ended well.
pub fn read_setup(printed: &str, ended: Result<(), String>) -> WindowsSetup {
  let lines: Vec<String> = printed
    .replace("\r\n", "\n")
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(str::to_owned)
    .collect();

  if lines.iter().any(|line| line == NOT_WINDOWS) {
    return WindowsSetup::NotWindows;
  }

  let placed = lines.iter().any(|line| line.starts_with("library "))
    && lines.iter().any(|line| line.starts_with("driver "));

  match ended {
    Ok(()) if placed => WindowsSetup::Done(lines),
    Ok(()) => WindowsSetup::Failed {
      detail: "the setup did not report both its library and its driver".to_owned(),
      lines,
    },
    Err(detail) => WindowsSetup::Failed { detail, lines },
  }
}

/// The setup run over a machine's own road, the hardware monitor's: the script as its shell takes it, its
/// whole answer, and how it ended.
pub async fn run_setup(source: &Source) -> WindowsSetup {
  let script = windows_setup_script();
  let script = if source.powershell() { script } else { windows_shell_line(&script) };
  let (lines, mut read) = mpsc::channel(64);
  let run = stream(source, &script, lines, CancellationToken::new());
  let gather = async {
    let mut printed = Vec::new();

    while let Some(line) = read.recv().await {
      printed.push(line);
    }

    printed
  };
  let (end, printed) = tokio::join!(run, gather);
  let ended = if end.ok { Ok(()) } else { Err(end.failure) };

  read_setup(&printed.join("\n"), ended)
}

/// The nodes named, or every node, set up one after another: a line printed for each, and how many failed.
pub async fn hardware_setup(store: &Store, names: &[String]) -> Result<(Vec<String>, usize), String> {
  let nodes = store.nodes().map_err(store_refusal)?;
  let unknown: Vec<&str> = names
    .iter()
    .filter(|name| !nodes.iter().any(|node| node.name == **name))
    .map(String::as_str)
    .collect();

  if !unknown.is_empty() {
    return Err(format!("no such node: {}", unknown.join(", ")));
  }

  let mut lines = Vec::new();
  let mut failed = 0;

  for node in nodes.iter().filter(|node| names.is_empty() || names.contains(&node.name)) {
    let Some(source) = source_of(node) else {
      lines.push(format!("{}: the row is missing a field its kind needs", node.name));
      failed += 1;
      continue;
    };

    match run_setup(&source).await {
      WindowsSetup::NotWindows => lines.push(format!("{}: no Windows, nothing to set up", node.name)),
      WindowsSetup::Done(said) => lines.push(format!("{}: {}", node.name, said.join("; "))),
      WindowsSetup::Failed {
        detail,
        lines: said,
      } => {
        let said = if said.is_empty() { String::new() } else { format!("\n  {}", said.join("\n  ")) };

        lines.push(format!("{}: failed — {detail}{said}", node.name));
        failed += 1;
      }
    }
  }

  Ok((lines, failed))
}
