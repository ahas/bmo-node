//! The hardware routes: a machine's latest reading from the recorder, or read once where the recorder has
//! nothing fresh, or none where its tracking is off; the live record's readings; one span of the history;
//! and the tracking switch and the CPU limit, refused in English where a request names no machine.

use reqwest::{Method, StatusCode};
use serde_json::json;
use store::nodes::SshLogin;

use crate::support::{Stand, failing_road, refused};

fn writer(stood: &crate::support::Stood) -> hardware::record::Writer {
  hardware::record::Writer::open(&stood.service.record).unwrap()
}

fn view() -> hardware::view::HardwareView {
  let text = std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hardware/fixtures/linux.txt"),
  )
  .unwrap();

  hardware::view::hardware_view(&hardware::probe::read_sections(&hardware::probe::sections(&text)).unwrap())
}

#[tokio::test]
async fn a_machine_s_latest_is_the_recorder_s_where_it_is_fresh_and_none_where_its_tracking_is_off() {
  let stood = Stand::default().up().await;
  let writer = writer(&stood);
  let now = store::now_seconds();

  writer.reading("local", now, &view());
  writer.written();

  let latest = stood.get("/hardware/local").await;

  assert_eq!(
    (
      latest["name"].clone(),
      latest["label"].clone(),
      latest["kind"].clone(),
      latest["tracking"].clone(),
      latest["readAt"].clone()
    ),
    (json!("local"), json!("Lab machine"), json!("local"), json!(true), json!(now))
  );
  assert_eq!(latest["hardware"]["host"], "example-host");
  assert_eq!(latest["failure"], json!(null));

  stood.post("/hardware/cpu-limit", json!({ "machine": "local", "celsius": 95 })).await;
  stood.post("/hardware/tracking", json!({ "machine": "local", "on": false })).await;

  let off = stood.get("/hardware/local").await;

  assert_eq!(
    (
      off["hardware"].clone(),
      off["failure"].clone(),
      off["tracking"].clone(),
      off["cpuLimit"].clone()
    ),
    (json!(null), json!(null), json!(false), json!(95.0))
  );
  assert_eq!(refused(&stood.get("/hardware/gone").await), "no-node");
}

#[tokio::test]
async fn a_tracked_machine_with_nothing_fresh_is_read_once_over_its_road() {
  let stood = Stand::default().up().await;

  stood
    .store()
    .add_node("lab", Some("Lab"), None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let read = stood.get("/hardware/lab").await;

  assert_eq!(
    (read["kind"].clone(), read["label"].clone(), read["hardware"]["host"].clone()),
    (json!("ssh"), json!("Lab"), json!("box"))
  );

  // A machine whose recorder last met a failure, fresh, answers that failure.
  let writer = writer(&stood);

  writer.failure("lab", store::now_seconds(), "the login was refused");
  writer.written();
  assert_eq!(stood.get("/hardware/lab").await["failure"], "the login was refused");

  // A node with no road, and a road that fails, say so.
  stood.store().add_agent("http://unlisted.example", "far").unwrap();
  assert_eq!(stood.get("/hardware/far").await["failure"], "the row is missing a field its kind needs");

  let failing = Stand {
    road: failing_road("someone@192.0.2.4: Permission denied (publickey)."),
    ..Stand::default()
  }
  .up()
  .await;

  assert_eq!(
    failing.get("/hardware/local").await["failure"],
    "someone@192.0.2.4: Permission denied (publickey)."
  );
}

#[tokio::test]
async fn the_live_record_s_readings_come_as_columns_of_the_metrics_asked_for() {
  let stood = Stand::default().up().await;
  let writer = writer(&stood);
  let now = store::now_seconds();

  writer.reading("local", now - 2, &view());
  writer.reading("local", now - 1, &view());
  writer.written();

  let all = stood.get("/hardware/local/live").await;
  let temps = stood.get(&format!("/hardware/local/live?since={}&metrics=temp", now - 2)).await;

  assert_eq!(all["at"], json!([now - 2, now - 1]));
  assert!(all["series"]["cpu|use"].is_array());
  assert_eq!(temps["at"], json!([now - 1]));
  assert!(temps["series"].as_object().unwrap().keys().all(|key| key.ends_with("|temp")));
}

#[tokio::test]
async fn a_span_of_the_history_is_cut_into_its_points_and_a_span_not_drawn_is_refused() {
  let stood = Stand::default().up().await;
  let hour = stood.get("/hardware/local/history").await;
  let week = stood.get("/hardware/local/history?span=7d").await;

  // A span ending now starts at a whole point, so it holds one point more where now falls inside one.
  assert!((60..=61).contains(&hour["at"].as_array().unwrap().len()));
  assert!((336..=337).contains(&week["at"].as_array().unwrap().len()));
  assert_eq!((hour["pointSeconds"].clone(), week["pointSeconds"].clone()), (json!(60), json!(1800)));

  let (status, answer) = stood.ask(Method::GET, "/hardware/local/history?span=2h", None).await;

  assert_eq!((status, answer["code"].clone()), (StatusCode::BAD_REQUEST, json!("bad-request")));
}

#[tokio::test]
async fn the_tracking_switch_and_the_cpu_limit_are_kept_per_machine_and_refused_in_english() {
  let stood = Stand::default().up().await;

  assert_eq!(
    stood.post("/hardware/tracking", json!({ "machine": "local", "on": false })).await,
    json!({ "machine": "local", "on": false })
  );
  assert!(!stood.store().tracking("local").unwrap());

  let (status, answer) =
    stood.ask(Method::POST, "/hardware/tracking", Some(json!({ "machine": "local" }))).await;

  assert_eq!(
    (status, answer["error"].clone()),
    (StatusCode::BAD_REQUEST, json!("machine (string) and on (boolean) are required"))
  );
  assert_eq!(
    refused(&stood.post("/hardware/tracking", json!({ "machine": "gone", "on": true })).await),
    "no-node"
  );
  assert_eq!(
    stood.post("/hardware/cpu-limit", json!({ "machine": "local", "celsius": 95.5 })).await,
    json!({ "machine": "local", "celsius": 95.5 })
  );
  assert_eq!(stood.store().setting("hardware_cpu_limit:local").unwrap().as_deref(), Some("95.5"));
  assert_eq!(
    stood.post("/hardware/cpu-limit", json!({ "machine": "local", "celsius": null })).await,
    json!({ "machine": "local", "celsius": null })
  );

  for body in [
    json!({ "machine": "local", "celsius": 130 }),
    json!({ "machine": "local", "celsius": "hot" }),
  ] {
    let (status, _) = stood.ask(Method::POST, "/hardware/cpu-limit", Some(body)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
  }
}
