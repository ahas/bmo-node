//! The tunnel door, run by `bmo-llm-bridge.service` (docs/llm-bridge.md).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use llm_bridge::door::{DOOR_LISTEN, Door, SSH_TARGET, router};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(
  name = "llm-bridge",
  about = format!(
    "Takes the SSH a node's tunnel carries inside HTTPS and joins it to this machine's SSH ({SSH_TARGET}).\n\
     Listens on {DOOR_LISTEN}, and takes a connection only when its bearer is a service key named in the \
     door's list (env/tunnel-door.keys)."
  )
)]
struct Cli {
  /// The data root the door's list stands under.
  #[arg(long, env = "BMO_DATA_DIR")]
  data_root: Option<PathBuf>,
  /// Where the door listens.
  #[arg(long, env = "LLM_BRIDGE_LISTEN", default_value = DOOR_LISTEN)]
  listen: String,
  /// What a taken WebSocket is piped to.
  #[arg(long, env = "LLM_BRIDGE_TARGET", default_value = SSH_TARGET)]
  target: String,
}

fn own_data_root() -> PathBuf {
  let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).unwrap_or_else(|| "/".into());
  PathBuf::from(home).join(".local/share/bmo/data")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let cli = Cli::parse();
  let data_root = cli.data_root.unwrap_or_else(own_data_root);
  let door = Arc::new(Door {
    list: data_root.join("env").join("tunnel-door.keys"),
    target: cli.target,
  });
  let listener = match TcpListener::bind(&cli.listen).await {
    Ok(listener) => listener,
    Err(fault) => {
      eprintln!("llm-bridge: could not listen on {}: {fault}", cli.listen);
      return ExitCode::FAILURE;
    }
  };
  match listener.local_addr() {
    Ok(at) => println!("llm-bridge: listening on {at}"),
    Err(_) => println!("llm-bridge: listening on {}", cli.listen),
  }
  match axum::serve(listener, router(door)).await {
    Ok(()) => ExitCode::SUCCESS,
    Err(fault) => {
      eprintln!("llm-bridge: {fault}");
      ExitCode::FAILURE
    }
  }
}
