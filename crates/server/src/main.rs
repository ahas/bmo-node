//! `bmo-node`: the node service and its command (docs/api.md, 「The command」).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use bmo_node_client::NodeClient;
use bmo_node_plugin::NodeServer;
use clap::{Parser, Subcommand};
use node_check::Machine;
use node_server::cli::{NodesArgs, hardware_setup, nodes};
use node_server::config::Config;
use node_server::{Service, app, start};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use store::Store;

#[derive(Parser)]
#[command(
  name = "bmo-node",
  version,
  about = "The node service: the node list, the node checks, the hardware monitor and this machine's standing"
)]
struct Cli {
  /// The configuration file; every step but `nouns` and `mcp` needs one. It may stand before or after the
  /// step's words, since `bmo` hands a noun's words on after the noun.
  #[arg(long, global = true)]
  config: Option<PathBuf>,
  #[command(subcommand)]
  step: Step,
}

#[derive(Subcommand)]
enum Step {
  /// Prints the nouns this command carries for `bmo`, one a line, each marked after a tab.
  Nouns,
  /// Moves the old archive's node tables from the item's export into this service's store (TODO #1923).
  Import {
    /// The export directory the item's export hand wrote.
    #[arg(long)]
    export: PathBuf,
    /// The store's SQLite file; made and migrated where it does not stand.
    #[arg(long)]
    database: PathBuf,
    /// Reads the export and reports what it would write, writing nothing.
    #[arg(long)]
    dry_run: bool,
  },
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
  /// Serves the harness plugin's tools over MCP on standard input and output, calling the service.
  Mcp {
    /// The service's base address; the configuration's listen address where none is given.
    #[arg(long, value_name = "url")]
    url: Option<String>,
    /// The service's bearer token; the configuration's where none is given.
    #[arg(long, env = "BMO_NODE_TOKEN", hide_env_values = true)]
    token: Option<String>,
  },
}

#[tokio::main]
async fn main() -> ExitCode {
  let cli = Cli::parse();
  let config = match cli.config.as_deref().map(Config::read).transpose() {
    Ok(config) => config,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      return ExitCode::from(2);
    }
  };

  match cli.step {
    Step::Nouns => {
      node_server::cli::NOUNS.iter().for_each(|noun| println!("{noun}"));

      return ExitCode::SUCCESS;
    }
    Step::Mcp { url, token } => return mcp(config.as_ref(), url, token).await,
    // The import reads its own arguments and never the deployment's configuration.
    Step::Import {
      export,
      database,
      dry_run,
    } => return imported(&export, &database, dry_run),
    _ => {}
  }

  let Some(config) = config else {
    eprintln!("bmo-node: this step needs --config <file>");
    return ExitCode::from(2);
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
    Step::Nouns => unreachable!("the nouns are printed above"),
    Step::Mcp { .. } => unreachable!("the MCP server is served above"),
    Step::Import { .. } => unreachable!("the import is answered before the configuration is read"),
  };

  match ended {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      ExitCode::FAILURE
    }
  }
}

/// The old archive's node tables moved into the store: the report printed, and a failure where the
/// count-and-sample compare found one.
fn imported(export: &Path, database: &Path, dry_run: bool) -> ExitCode {
  match store::import::run(database, export, dry_run) {
    Ok(report) => {
      for line in report.lines() {
        println!("{line}");
      }
      if report.sample.failures.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
    }
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
  let service = Arc::new(
    Service::new(store, config.token.clone(), data_root, config.machine_label.clone())
      .with_scope_variable(config.scope_variable.clone()),
  );
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

/// The harness plugin's MCP server on standard input and output, calling the service at the address and
/// with the token given, or those the configuration names.
async fn mcp(config: Option<&Config>, url: Option<String>, token: Option<String>) -> ExitCode {
  let Some(url) = url.or_else(|| config.map(Config::local_address)) else {
    eprintln!("bmo-node: mcp needs --url <url> or --config <file>");
    return ExitCode::from(2);
  };
  let Some(token) = token.or_else(|| config.map(|config| config.token.clone())) else {
    eprintln!("bmo-node: mcp needs --token, BMO_NODE_TOKEN or --config <file>");
    return ExitCode::from(2);
  };
  let client = match NodeClient::new(&url, token) {
    Ok(client) => client,
    Err(why) => {
      eprintln!("bmo-node: {why}");
      return ExitCode::from(2);
    }
  };
  let served = match NodeServer::new(client).serve(stdio()).await {
    Ok(served) => served,
    Err(why) => {
      eprintln!("bmo-node: the MCP session did not start: {why}");
      return ExitCode::FAILURE;
    }
  };

  match served.waiting().await {
    Ok(_) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-node: the MCP session ended in a failure: {why}");
      ExitCode::FAILURE
    }
  }
}
