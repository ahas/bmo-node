//! The node-exec tool's executable: one call handed by the tool host on standard input, answered on standard
//! output (bmo-pocket's `docs/api.md`, 「The call protocol」).

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
  node_exec_tool::run_call().await
}
