//! A peer of the tool stopped in the middle of the work and started again: a hub stopped while a command
//! runs is said as a command with no known end, with what it said before; a call while the hub is down is the
//! hub not answering; and the hub started again on the same address runs the next call. Agency stopped lets
//! nobody but the administrator through, and started again lets the node's owner through once more. Each call
//! is a fresh process of the tool's, so nothing of a stopped peer is held from one call to the next.

use std::net::SocketAddr;

use bmo_node_client::nodes::LoginShell;
use node_exec_tool::answer;
use node_exec_tool::caller::{Agency, judge};
use serde_json::json;

use super::{frame, handed, held_hub, hub_target, out, owned, serve_at, session};

#[tokio::test]
async fn a_hub_stopped_in_the_middle_of_a_command_and_started_again_runs_the_next_call() {
  let stood = owned().await;
  let held = held_hub(frame("started", json!({ "id": "c1" })) + &out("stdout", b"step one\n", 0)).await;
  let target = hub_target("pc-four", &held.url, "agent_w", LoginShell::Powershell);

  stood.world.targets.lock().unwrap().insert("pc-four".into(), target);

  let asked = handed("pc-agent", &[], json!({ "node": "pc-four", "command": "long-step" }));
  let (stop, streaming) = (held.stop, held.streaming);
  let stopping = async move {
    streaming.await.unwrap();
    stop.send(()).unwrap();
  };
  let (cut, ()) = tokio::join!(answer(&asked, &stood.ground), stopping);

  assert!(cut.is_error);
  assert_eq!(
    cut.text(),
    "The command on pc-four has no known end: the hub's stream broke off.\n--- stdout before that\nstep one\n"
  );

  let down = answer(&asked, &stood.ground).await;

  assert!(down.is_error);
  assert!(
    down
      .text()
      .starts_with("The command on pc-four has no known end: the hub did not answer:"),
    "{}",
    down.text()
  );

  *stood.world.frames.lock().unwrap() = vec![
    frame("started", json!({ "id": "c2" })),
    out("stdout", b"step two\n", 0),
    frame("exit", json!({ "code": 0, "signal": null, "canceled": false })),
  ];
  let (_, again) = serve_at(&stood.world, held.addr).await;
  let ran = answer(&asked, &stood.ground).await;

  again.abort();
  assert!(!ran.is_error, "{}", ran.text());
  assert_eq!(ran.structured.as_ref().unwrap()["stdout"], json!("step two\n"));
  assert_eq!(ran.structured.as_ref().unwrap()["exitCode"], json!(0));
}

#[tokio::test]
async fn agency_stopped_lets_nobody_but_the_administrator_through_and_started_again_lets_the_owner_in() {
  let stood = owned().await;
  let (addr, agency) = serve_at(&stood.world, SocketAddr::from(([127, 0, 0, 1], 0))).await;
  let url = format!("http://{addr}");
  let owner = session("pc-agent", &[]);

  assert_eq!(judge(&owner, "pc-four", Ok(&Agency::new(&url, "token"))).await, Ok(()));

  agency.abort();
  assert!(agency.await.unwrap_err().is_cancelled());

  let refused = judge(&owner, "pc-four", Ok(&Agency::new(&url, "token"))).await.unwrap_err();

  assert!(refused.starts_with("Whose node pc-four is could not be read"), "{refused}");
  assert_eq!(
    judge(&session("house", &["node.admin"]), "pc-four", Ok(&Agency::new(&url, "token"))).await,
    Ok(())
  );

  let (_, again) = serve_at(&stood.world, addr).await;
  let let_in = judge(&owner, "pc-four", Ok(&Agency::new(&url, "token"))).await;

  again.abort();
  assert_eq!(let_in, Ok(()));
}
