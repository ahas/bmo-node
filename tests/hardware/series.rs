//! What a screen is handed of the recorded figures: a reading's figures named by their piece of hardware,
//! a span cut into its own points, a short span out of the live record, a store that never recorded, and
//! the live record's readings as columns with only the metrics asked for.

use hardware::probe::{Fan, Gpu, Memory, Sensor, Snapshot, Watts};
use hardware::record::{Reader, Writer};
use hardware::series::{HISTORY_SPANS, LiveRange, gather_live, read_history, read_span, span};
use hardware::storage::{StorageDevice, StorageVolume};
use hardware::view::{hardware_view, history_sample};
use serde_json::Value;
use store::Store;

fn snapshot(usage: f64) -> Snapshot {
  Snapshot {
    host: Some("box".into()),
    kernel: Some("6.6".into()),
    wsl: false,
    os: None,
    host_os: None,
    sensor_library: None,
    uptime_seconds: Some(1.0),
    load: None,
    memory: Some(Memory {
      total_kb: 33_554_432.0,
      available_kb: 20_656_948.0,
      swap_total_kb: 0.0,
      swap_free_kb: 0.0,
    }),
    cpu_model: None,
    cpu_usage: Some(usage),
    clock_source: None,
    threads: vec![],
    cpu_watts: Some(Watts::Denied),
    sensors: vec![Sensor {
      chip: "k10temp".into(),
      device: None,
      label: "Tctl".into(),
      celsius: 61.25,
      limit_celsius: None,
    }],
    fans: vec![Fan {
      chip: "nct6797".into(),
      label: "fan2".into(),
      rpm: 800.0,
    }],
    gpus: vec![Gpu {
      index: 0.0,
      name: "GPU".into(),
      usage: Some(0.5),
      celsius: Some(40.0),
      mhz: None,
      max_mhz: None,
      memory_mhz: None,
      watts: Some(20.0),
      watt_limit: Some(200.0),
      memory_used_mb: Some(2048.0),
      memory_total_mb: Some(8192.0),
      fan_percent: None,
      limit_celsius: None,
      throttling: None,
    }],
    storage: vec![StorageDevice {
      key: "nvme0n1".into(),
      name: "SSD".into(),
      kind: Some("nvme"),
      size_bytes: Some(1.0),
      celsius: Some(45.0),
      limit_celsius: None,
      volumes: vec![StorageVolume {
        mount: "/".into(),
        fs_type: Some("ext4".into()),
        total_kb: 31_457_280.0,
        used_kb: 10_485_760.0,
        available_kb: 20_971_520.0,
      }],
    }],
    services: (None, None),
  }
}

#[test]
fn names_each_figure_by_its_piece_of_hardware_keeps_it_in_its_unit_and_leaves_out_what_was_not_read() {
  let sample = history_sample(&hardware_view(&snapshot(0.123)));
  let read: Vec<(String, f64)> =
    sample.iter().map(|(key, value)| (key.clone(), value.as_f64().unwrap())).collect();
  let mut expected: Vec<(String, f64)> = [
    ("cpu|use", 12.3),
    ("cpu|temp", 61.3),
    ("memory|gb", 12.3),
    ("gpu0|use", 50.0),
    ("gpu0|temp", 40.0),
    ("gpu0|watts", 20.0),
    ("gpu0:vram|gb", 2.0),
    ("storage:nvme0n1|gb", 10.0),
    ("storage:nvme0n1|temp", 45.0),
    ("fan:nct6797:fan2|rpm", 800.0),
  ]
  .into_iter()
  .map(|(key, value)| (key.to_owned(), value))
  .collect();
  let mut read = read;

  expected.sort_by(|a, b| a.0.cmp(&b.0));
  read.sort_by(|a, b| a.0.cmp(&b.0));
  assert_eq!(read, expected);
}

#[test]
fn a_span_ends_now_and_is_cut_into_its_own_points() {
  let store = Store::in_memory().unwrap();
  let now = 40 * 86_400;
  let points: Vec<(usize, i64)> = HISTORY_SPANS
    .iter()
    .map(|(_, seconds, point)| {
      let range = read_span(&store, None, "box", *seconds, *point, now).unwrap();

      (range.at.len(), range.point_seconds)
    })
    .collect();

  assert_eq!(points, [(60, 5), (60, 15), (60, 60), (72, 300), (288, 300), (336, 1_800), (360, 7_200)]);
  assert_eq!(span("1h"), Some((3_600, 60)));
  assert_eq!(span("2h"), None);
}

#[test]
fn a_short_span_is_drawn_out_of_the_live_record_s_readings() {
  let now = 1_000;
  let since = now - 300;
  let range = LiveRange {
    at: vec![since, since + 1, since + 6, now],
    series: [("cpu|use".to_owned(), vec![Some(10.0), Some(30.0), Some(50.0), None])]
      .into_iter()
      .collect(),
  };
  let history = gather_live(&range, since, now, 5);
  let band = &history.series["cpu|use"];

  assert_eq!(history.at[0], 700);
  assert_eq!(band.mean[..3], [Some(20.0), Some(50.0), None]);
  assert_eq!(band.max[0], Some(30.0));
}

#[test]
fn a_store_that_never_recorded_reads_as_an_empty_range() {
  let range = read_history(&Store::in_memory().unwrap(), "box", 0, 3_600, 1_800).unwrap();

  assert_eq!((range.at, range.point_seconds, range.series.len()), (vec![0, 1_800], 1_800, 0));
}

#[test]
fn a_rolled_up_half_hour_and_the_minutes_not_rolled_up_yet_both_stand_in_a_wide_span() {
  let store = Store::in_memory().unwrap();

  store
    .put_rollups(&[store::history::Row {
      machine: "box".into(),
      at: 0,
      figures: r#"{"cpu|use":[10,30,40,2]}"#.into(),
    }])
    .unwrap();
  // A minute of the rolled-up half hour stands for nothing more; one of the next half hour stands.
  store
    .put_samples("box", &[(60, r#"{"cpu|use":99}"#.into()), (1_860, r#"{"cpu|use":50}"#.into())])
    .unwrap();

  let band = &read_history(&store, "box", 0, 3_600, 1_800).unwrap().series["cpu|use"];

  assert_eq!(
    (band.min.clone(), band.max.clone(), band.mean.clone()),
    (vec![Some(10.0), Some(50.0)], vec![Some(30.0), Some(50.0)], vec![Some(20.0), Some(50.0)])
  );
}

fn view(usage: f64) -> hardware::view::HardwareView {
  hardware_view(&snapshot(usage))
}

#[test]
fn gives_a_machine_s_readings_after_an_instant_as_columns_and_only_the_metrics_asked_for() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();
  let mut no_temperature = snapshot(0.2);

  no_temperature.sensors = vec![];
  writer.reading("box", 100, &view(0.1));
  writer.reading("box", 101, &hardware_view(&no_temperature));
  writer.reading("box", 102, &view(0.3));
  writer.reading("other", 101, &view(0.9));
  writer.written();

  let reader = Reader::open(&path).unwrap();
  let temps = reader.range("box", 100, &["temp".to_owned()]).unwrap();

  assert_eq!(temps.at, [101, 102]);
  assert_eq!(temps.series["cpu|temp"], [None, Some(61.3)]);
  assert!(temps.series.keys().all(|key| key.ends_with("|temp")));
  assert_eq!(
    reader.range("box", 0, &[]).unwrap().series["cpu|use"],
    [Some(10.0), Some(20.0), Some(30.0)]
  );
}

#[test]
fn removes_what_is_older_than_its_reach_then_the_oldest_beyond_the_most_it_may_hold() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();

  for at in [100, 200, 300, 400] {
    writer.reading("box", at, &view(0.5));
  }

  writer.prune_within(450, 300, i64::MAX);
  writer.written();

  let reader = Reader::open(&path).unwrap();
  let ats =
    |reader: &Reader| reader.samples("box", 0, 1_000).unwrap().iter().map(|one| one.at).collect::<Vec<_>>();

  assert_eq!(ats(&reader), [200, 300, 400]);

  // Room for two rows' text keeps the newest two.
  let one = Value::Object(reader.samples("box", 400, 401).unwrap().remove(0).sample)
    .to_string()
    .len() as i64;

  writer.prune_within(450, 300, 2 * one);
  writer.written();
  assert_eq!(ats(&reader), [300, 400]);
}
