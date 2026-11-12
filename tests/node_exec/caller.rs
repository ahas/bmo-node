//! Who may run a command on which node: the node's owner and the administrator, and nobody else, each refusal
//! naming the node and its owner.

use node_exec_tool::caller::{Agency, judge};
use serde_json::json;

use super::{session, stood};

#[tokio::test]
async fn a_supervisor_reaches_its_own_account_s_node_and_not_another_s() {
  let stood = stood().await;

  stood.world.supervisors.lock().unwrap().insert("pc-agent".into(), json!(4));
  stood.world.person_nodes.lock().unwrap().extend([
    json!({ "ownerAccount": "4", "node": "pc-four", "setAt": 1 }),
    json!({ "ownerAccount": "7", "node": "pc-seven", "setAt": 2 }),
  ]);

  let agency = stood.ground.agency.as_ref().unwrap();
  let caller = session("pc-agent", &[]);

  assert_eq!(judge(&caller, "pc-four", Ok(agency)).await, Ok(()));
  assert_eq!(
    judge(&caller, "pc-seven", Ok(agency)).await,
    Err(
      "The node pc-seven is account 7's; this session's agent pc-agent is account 4's and holds no \
       administrator's grant (node.admin)."
        .into()
    )
  );
}

#[tokio::test]
async fn a_role_agent_s_account_is_read_and_a_double_answers_for_its_supervisor() {
  let stood = stood().await;

  stood
    .world
    .role_agents
    .lock()
    .unwrap()
    .push(json!({ "name": "helper", "role": "worker", "ownerAccount": 7 }));
  stood.world.supervisors.lock().unwrap().insert("pc-agent".into(), json!(4));
  stood
    .world
    .person_nodes
    .lock()
    .unwrap()
    .push(json!({ "ownerAccount": "7", "node": "pc-seven", "setAt": 2 }));

  let agency = stood.ground.agency.as_ref().unwrap();
  let mut double = session("pc-agent-double", &[]);

  double.labels.insert("doubleOf".into(), json!("pc-agent"));

  assert_eq!(judge(&session("helper", &[]), "pc-seven", Ok(agency)).await, Ok(()));
  assert_eq!(
    judge(&double, "pc-seven", Ok(agency)).await,
    Err(
      "The node pc-seven is account 7's; this session's agent pc-agent is account 4's and holds no \
       administrator's grant (node.admin)."
        .into()
    )
  );
}

#[tokio::test]
async fn an_agent_of_no_account_reaches_no_node_and_a_node_of_no_person_is_the_administrator_s_alone() {
  let stood = stood().await;

  stood.world.supervisors.lock().unwrap().insert("house".into(), json!(null));
  stood
    .world
    .person_nodes
    .lock()
    .unwrap()
    .push(json!({ "ownerAccount": "4", "node": "pc-four", "setAt": 1 }));

  let agency = stood.ground.agency.as_ref().unwrap();

  assert_eq!(
    judge(&session("house", &[]), "pc-four", Ok(agency)).await,
    Err(
      "The node pc-four is account 4's; this session's agent house belongs to no account and holds no \
       administrator's grant (node.admin)."
        .into()
    )
  );
  assert_eq!(
    judge(&session("worker", &[]), "build-box", Ok(agency)).await,
    Err(
      "The node build-box is no person's node and is the administrator's alone; this session's agent worker \
       belongs to no account and holds no administrator's grant (node.admin)."
        .into()
    )
  );
}

#[tokio::test]
async fn the_administrator_s_grant_reaches_every_node_without_asking_agency() {
  let unreachable = "agency is not configured".to_owned();

  assert_eq!(judge(&session("house", &["node.admin"]), "pc-four", Err(&unreachable)).await, Ok(()));
}

#[tokio::test]
async fn an_owner_that_cannot_be_read_lets_nobody_but_the_administrator_through() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let gone = format!("http://{}", listener.local_addr().unwrap());

  drop(listener);

  let refused = judge(&session("pc-agent", &[]), "pc-four", Ok(&Agency::new(gone, "token")))
    .await
    .unwrap_err();

  assert!(
    refused.starts_with(
      "Whose node pc-four is could not be read, and the session holds no administrator's grant"
    ),
    "{refused}"
  );
}
