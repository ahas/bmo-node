//! The machines: a machine's latest reading from the recorder, or read once where the recorder has nothing
//! fresh, or none where its tracking is off; every machine as the recorder holds it; the tracking switch
//! and the CPU limit changed; this machine's standing; and its past readings.

use reqwest::Method;
use serde_json::json;
use store::nodes::SshLogin;

use crate::support::{SCOPE, Stand, failing_road};

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

  let latest = stood.get("/machines/local").await;

  assert_eq!(
    (
      latest["name"].clone(),
      latest["label"].clone(),
      latest["kind"].clone(),
      latest["tracking"].clone(),
      latest["readAt"].clone()
    ),
    (json!("local"), json!("Lab machine"), json!("local"), json!(true), json!(now * 1000))
  );
  assert_eq!(latest["hardware"]["host"], "example-host");
  assert_eq!(latest["failure"], json!(null));
  assert_eq!(latest["standing"]["reading"], json!(null), "no minute look has kept a standing yet");

  let off = stood.patch("/machines/local", json!({ "tracking": false, "cpuLimit": 95 })).await;

  assert_eq!(
    (
      off["hardware"].clone(),
      off["failure"].clone(),
      off["readAt"].clone(),
      off["tracking"].clone(),
      off["cpuLimit"].clone()
    ),
    (json!(null), json!(null), json!(null), json!(false), json!(95.0))
  );
  assert!(!stood.store().tracking("local").unwrap());
  assert_eq!(stood.refused(Method::GET, "/machines/gone", None).await, "not-found");
}

#[tokio::test]
async fn a_tracked_machine_with_nothing_fresh_is_read_once_over_its_road() {
  let stood = Stand::default().up().await;

  stood
    .store()
    .add_node("lab", Some("Lab"), None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();

  let read = stood.get("/machines/lab").await;

  assert_eq!(
    (
      read["kind"].clone(),
      read["label"].clone(),
      read["hardware"]["host"].clone(),
      read["standing"].clone()
    ),
    (json!("ssh"), json!("Lab"), json!("box"), json!(null))
  );

  // A machine whose recorder last met a failure, fresh, answers that failure.
  let writer = writer(&stood);

  writer.failure("lab", store::now_seconds(), "the login was refused");
  writer.written();
  assert_eq!(stood.get("/machines/lab").await["failure"], "the login was refused");

  // A node with no road, and a road that fails, say so.
  stood.store().add_agent("http://unlisted.example", "far").unwrap();
  assert_eq!(stood.get("/machines/far").await["failure"], "the row is missing a field its kind needs");

  let failing = Stand {
    road: failing_road("someone@192.0.2.4: Permission denied (publickey)."),
    ..Stand::default()
  }
  .up()
  .await;

  assert_eq!(
    failing.get("/machines/local").await["failure"],
    "someone@192.0.2.4: Permission denied (publickey)."
  );
}

#[tokio::test]
async fn every_machine_is_answered_as_the_recorder_holds_it_reading_none_again() {
  let stood = Stand {
    road: failing_road("no machine is read for the list"),
    ..Stand::default()
  }
  .up()
  .await;
  let writer = writer(&stood);

  stood
    .store()
    .add_node("lab", None, None, SshLogin::new("192.0.2.4", "someone"))
    .unwrap();
  stood.store().add_agent(store::HUB_SEED.0, "box").unwrap();
  // A reading of long ago is still what the recorder holds.
  writer.reading("lab", 1_000, &view());
  writer.written();

  let page = stood.get("/machines").await;
  let items = page["items"].as_array().unwrap();

  assert_eq!(
    items
      .iter()
      .map(|one| (one["name"].as_str().unwrap(), one["kind"].as_str().unwrap()))
      .collect::<Vec<_>>(),
    [("local", "local"), ("lab", "ssh"), ("box", "hub")]
  );
  assert_eq!(
    (items[1]["readAt"].clone(), items[1]["hardware"]["host"].clone()),
    (json!(1_000_000), json!("example-host"))
  );
  assert_eq!((items[0]["readAt"].clone(), items[0]["failure"].clone()), (json!(null), json!(null)));
  assert!(items[0]["standing"].is_object() && items[1]["standing"].is_null());
}

#[tokio::test]
async fn the_tracking_switch_and_the_cpu_limit_are_kept_per_machine_and_refused_in_english() {
  let stood = Stand::default().up().await;

  assert_eq!(stood.patch("/machines/local", json!({ "cpuLimit": 95.5 })).await["cpuLimit"], 95.5);
  assert_eq!(stood.store().setting("hardware_cpu_limit:local").unwrap().as_deref(), Some("95.5"));
  assert_eq!(stood.patch("/machines/local", json!({ "tracking": true })).await["cpuLimit"], 95.5);
  assert_eq!(
    stood.patch("/machines/local", json!({ "cpuLimit": null })).await["cpuLimit"],
    json!(null)
  );
  assert_eq!(
    stood.refused(Method::PATCH, "/machines/local", Some(json!({ "cpuLimit": 130 }))).await,
    "bad-field"
  );
  assert_eq!(
    stood
      .refused(Method::PATCH, "/machines/local", Some(json!({ "cpuLimit": "hot" })))
      .await,
    "bad-request"
  );
  assert_eq!(
    stood.refused(Method::PATCH, "/machines/gone", Some(json!({ "tracking": true }))).await,
    "not-found"
  );
}

#[tokio::test]
async fn this_machine_s_standing_is_the_minute_look_s_with_each_process_s_scope() {
  let stood = Stand::default().up().await;
  let proc = &stood.service.proc_root;
  let process = proc.join("100");

  std::fs::create_dir_all(&process).unwrap();
  std::fs::write(process.join("status"), "Name:\tpython3\nVmRSS:\t900000 kB\nVmSwap:\t0 kB\n").unwrap();
  std::fs::write(process.join("cmdline"), "python3").unwrap();
  std::fs::write(process.join("environ"), format!("{SCOPE}=lab-run\0")).unwrap();
  // The host's free disk is not this test's subject: hold the disk alert below what any disk gives.
  stood.store().set_setting("machine_limit:alert_disk_free_gb", "0.001").unwrap();
  node_server::machine_look(&stood.service).await.unwrap();

  let standing = stood.get("/machines/local").await["standing"].clone();

  assert_eq!(standing["reading"]["threads"], 24);
  assert_eq!(standing["scopes"], json!([{ "scope": "lab-run", "megabytes": 879, "browsers": 0 }]));
  assert_eq!(standing["largest"][0]["scope"], "lab-run");
  // Memory full at 25% stands over the alert line, but not yet for the alert minutes.
  assert_eq!(standing["alerts"], json!([]));
}

#[tokio::test]
async fn a_machine_s_readings_are_paged_out_of_the_rows_their_span_is_drawn_from() {
  let stood = Stand::default().up().await;
  let writer = writer(&stood);
  let now = store::now_seconds();

  writer.reading("local", now - 2, &view());
  writer.reading("local", now - 1, &view());
  writer.written();

  let seconds = stood.get("/machines/local/readings").await;
  let items = seconds["items"].as_array().unwrap();

  assert_eq!(
    items
      .iter()
      .map(|one| (one["at"].as_i64().unwrap(), one["spanMs"].as_i64().unwrap()))
      .collect::<Vec<_>>(),
    [((now - 2) * 1000, 1000), ((now - 1) * 1000, 1000)]
  );
  assert!(items[0]["cpu"]["usePct"]["mean"].is_number());
  assert_eq!(items[0]["machineName"], "local");

  let after = stood.get(&format!("/machines/local/readings?after={}&limit=5", (now - 2) * 1000)).await;

  assert_eq!(after["items"].as_array().unwrap().len(), 1);
  assert_eq!(
    stood.get("/machines/local/readings?spanMs=60000").await,
    json!({ "items": [], "more": false })
  );
  assert_eq!(
    stood.refused(Method::GET, "/machines/local/readings?spanMs=1500", None).await,
    "bad-request"
  );
  assert_eq!(stood.refused(Method::GET, "/machines/gone/readings", None).await, "not-found");
}
