//! The harness plugin: `bmo-node mcp --url <url>` starts an MCP server on standard input and output that
//! lists exactly the document's eight tools, and each call reaches the stood service through the client,
//! behind the token, a refusal coming back as a tool error with the service's code. The command runs as its
//! own process, spoken to over its pipes, and is taken down before the test ends.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use store::nodes::SshLogin;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::server::support::{CONTRACT, Stand, TOKEN};
use crate::support::binary::built;

/// How long the server may take to answer one request before the test fails; a guard, never a wait.
const PATIENCE: Duration = Duration::from_secs(30);

/// The command, spoken to line by line.
struct Server {
  child: Child,
  input: ChildStdin,
  output: Lines<BufReader<ChildStdout>>,
  next: u64,
}

impl Server {
  fn start(url: &str) -> Self {
    let mut child = Command::new(built("node-server", "bmo-node"))
      .args(["mcp", "--url", url])
      .env("BMO_NODE_TOKEN", TOKEN)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .kill_on_drop(true)
      .spawn()
      .expect("the command starts");
    let input = child.stdin.take().expect("the command's input is piped");
    let output = BufReader::new(child.stdout.take().expect("the command's output is piped")).lines();

    Self {
      child,
      input,
      output,
      next: 1,
    }
  }

  async fn send(&mut self, message: Value) {
    let mut line = message.to_string();

    line.push('\n');
    self.input.write_all(line.as_bytes()).await.expect("the server reads its input");
    self.input.flush().await.expect("the server reads its input");
  }

  /// One request, answered with the result of the answer bearing its id.
  async fn ask(&mut self, method: &str, params: Value) -> Value {
    let id = self.next;

    self.next += 1;
    self
      .send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
      .await;

    loop {
      let line = tokio::time::timeout(PATIENCE, self.output.next_line())
        .await
        .expect("the server answers in time")
        .expect("the server's output reads")
        .expect("the server answers before it closes its output");
      let answer: Value = serde_json::from_str(&line).expect("the server writes JSON lines");

      if answer["id"] == id {
        assert!(answer["error"].is_null(), "{method} is answered without a protocol error: {answer}");
        return answer["result"].clone();
      }
    }
  }

  async fn call(&mut self, tool: &str, arguments: Value) -> Value {
    self.ask("tools/call", json!({ "name": tool, "arguments": arguments })).await
  }

  async fn stop(mut self) {
    drop(self.input);
    self.child.kill().await.expect("the command is taken down");
  }
}

#[tokio::test]
async fn the_server_lists_exactly_the_documented_tools_and_each_calls_its_route() {
  let stood = Stand::default().up().await;

  stood
    .store()
    .add_node("lab", Some("Lab"), None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let mut server = Server::start(&stood.base);
  let started = server
    .ask(
      "initialize",
      json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "tester", "version": "1" } }),
    )
    .await;

  assert_eq!(started["serverInfo"]["name"], "bmo-node");
  server.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;

  let listed = server.ask("tools/list", json!({})).await;
  let names: Vec<&str> = listed["tools"]
    .as_array()
    .unwrap()
    .iter()
    .map(|tool| tool["name"].as_str().unwrap())
    .collect();
  let documented: Vec<&str> = CONTRACT["x-tools"]
    .as_array()
    .unwrap()
    .iter()
    .map(|tool| tool["name"].as_str().unwrap())
    .collect();

  assert_eq!(
    names,
    [
      "node_nodes",
      "node_node",
      "node_target",
      "node_check",
      "node_machines",
      "node_machine",
      "node_readings",
      "node_memory"
    ]
  );
  assert_eq!(names, documented);
  assert!(
    listed["tools"]
      .as_array()
      .unwrap()
      .iter()
      .all(|tool| tool["outputSchema"]["type"] == "object")
  );

  let node = server.call("node_node", json!({ "nodeName": "lab" })).await;

  assert_eq!(
    (node["isError"].clone(), node["structuredContent"]["label"].clone()),
    (json!(false), json!("Lab"))
  );

  let page = server.call("node_nodes", json!({ "limit": 1 })).await;

  assert_eq!(page["structuredContent"]["items"][0]["name"], "lab");

  let target = server.call("node_target", json!({ "nodeName": "lab" })).await;

  assert!(
    target["structuredContent"]["sshArgs"]
      .as_array()
      .unwrap()
      .contains(&json!("someone@192.0.2.4"))
  );

  let machine = server.call("node_machine", json!({ "machineName": "local" })).await;

  assert_eq!(machine["structuredContent"]["kind"], "local");

  let readings = server.call("node_readings", json!({ "machineName": "local", "spanMs": 60000 })).await;

  assert_eq!(readings["structuredContent"], json!({ "items": [], "more": false }));

  let gone = server.call("node_node", json!({ "nodeName": "gone" })).await;
  let said: Value = serde_json::from_str(gone["content"][0]["text"].as_str().unwrap()).unwrap();

  assert_eq!(
    (gone["isError"].clone(), said["status"].clone(), said["code"].clone()),
    (json!(true), json!(404), json!("not-found"))
  );

  let wrong = server.call("node_target", json!({ "node": "lab" })).await;
  let said: Value = serde_json::from_str(wrong["content"][0]["text"].as_str().unwrap()).unwrap();

  assert_eq!((wrong["isError"].clone(), said["code"].clone()), (json!(true), json!("bad-request")));
  server.stop().await;
}
