//! `bmo-node`: the node service and its command (docs/api.md, 「The command」).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
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
  };

  match ended {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-node: {why}");
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
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|why| why.to_string())?;
  let answer = client
    .get(format!("{}/health", config.local_address()))
    .bearer_auth(&config.token)
    .send()
    .await
    .map_err(|why| format!("the service did not answer: {why}"))?;
  let health: serde_json::Value =
    answer.json().await.map_err(|why| format!("the health did not read: {why}"))?;

  if health["ready"] == true { Ok(()) } else { Err("the service is not ready".into()) }
}
