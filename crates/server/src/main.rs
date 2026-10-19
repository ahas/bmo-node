//! `bmo-node`: the node service and its command (docs/api.md, 「The command」).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use bmo_node_client::NodeClient;
use clap::{Parser, Subcommand};
use node_check::Machine;
use node_server::cli::{NodesArgs, hardware_setup, nodes};
use node_server::config::Config;
use node_server::{Service, app, start};
use store::Store;

#[derive(Parser)]
#[command(
  name = "bmo-node",
  about = "The node service: the node list, the node checks, the hardware monitor and this machine's room"
)]
struct Cli {
  /// The configuration file.
  #[arg(long)]
  config: PathBuf,
  #[command(subcommand)]
  step: Step,
}

#[derive(Subcommand)]
enum Step {
  /// Runs the migrations, then the routes, the check pass, the recorder and the machine watch.
  Serve,
  /// Asks the service the configuration names whether it is ready; ends 0 where it is.
  Health,
  /// Lists and changes the nodes against the store (docs/nodes.md, 「Registering and changing a node」).
  Node(Box<NodesArgs>),
  /// Puts the Windows sensor library and its driver in place on the nodes named, or on every node.
  HardwareSetup {
    /// The nodes to set up; every node where none is named.
    nodes: Vec<String>,
  },
}

#[tokio::main]
async fn main() -> ExitCode {
  let cli = Cli::parse();
  let config = match Config::read(&cli.config) {
    Ok(config) => config,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      return ExitCode::from(2);
    }
  };

  let ended = match cli.step {
    Step::Serve => serve(config).await,
    Step::Health => health(config).await,
    Step::Node(args) => {
      return command(&config, |store| async move {
        nodes(&store, &Machine::local(), *args).await.map(|lines| (lines, 0))
      })
      .await;
    }
    Step::HardwareSetup { nodes } => {
      return command(&config, |store| async move { hardware_setup(&store, &nodes).await }).await;
    }
  };

  match ended {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      ExitCode::FAILURE
    }
  }
}

/// One command over the store the configuration names: its lines printed, a refusal said on the standard
/// error, and the exit 1 where it was refused or any part of it failed.
async fn command<Run, Ran>(config: &Config, run: Run) -> ExitCode
where
  Run: FnOnce(Store) -> Ran,
  Ran: Future<Output = Result<(Vec<String>, usize), String>>,
{
  let store = match Store::open_in(&config.data_root()) {
    Ok(store) => store,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      return ExitCode::FAILURE;
    }
  };

  match run(store).await {
    Ok((lines, failed)) => {
      for line in lines {
        println!("{line}");
      }

      if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
    }
    Err(why) => {
      eprintln!("{why}");
      ExitCode::FAILURE
    }
  }
}

async fn serve(config: Config) -> Result<(), String> {
  let data_root = config.data_root();
  let store = Arc::new(Store::open_in(&data_root).map_err(|why| why.to_string())?);
  let service =
    Arc::new(Service::new(store, config.token.clone(), data_root, config.machine_label.clone()));
  let listener = tokio::net::TcpListener::bind(&config.listen)
    .await
    .map_err(|why| format!("could not listen on {}: {why}", config.listen))?;

  match listener.local_addr() {
    Ok(at) => println!("bmo-node: listening on {at}"),
    Err(_) => println!("bmo-node: listening on {}", config.listen),
  }

  let _loops = start(service.clone())?;

  axum::serve(listener, app(service)).await.map_err(|why| why.to_string())
}

async fn health(config: Config) -> Result<(), String> {
  let http = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|why| why.to_string())?;
  let client = NodeClient::with_http(&config.local_address(), &config.token, http)?;
  let health = client.health().await.map_err(|why| why.to_string())?;

  if health.ready { Ok(()) } else { Err("the service is not ready".into()) }
}
