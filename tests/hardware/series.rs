//! What a caller is handed of the recorded figures: a reading's figures named by their piece of hardware,
//! each span drawn out of the rows that hold it, the live record's seconds paged after an instant, a store
//! that never recorded, and the figures laid out per piece of hardware.

use std::collections::BTreeMap;

use hardware::probe::{Fan, Gpu, Memory, Sensor, Snapshot, Watts};
use hardware::record::{Reader, Writer};
use hardware::series::{Asked, Band, Rows, live_reading, read_readings, reading_of};
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
      shmem_kb: None,
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
    ("memory:available|gb", 19.7),
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
fn each_span_is_drawn_out_of_the_rows_that_hold_it_whole() {
  assert_eq!(Rows::of_span(1_000), Some(Rows::Live));
  assert_eq!(Rows::of_span(15_000), Some(Rows::Live));
  assert_eq!(Rows::of_span(60_000), Some(Rows::Minutes));
  assert_eq!(Rows::of_span(300_000), Some(Rows::Minutes));
  assert_eq!(Rows::of_span(1_800_000), Some(Rows::HalfHours));
  assert_eq!(Rows::of_span(7_200_000), Some(Rows::HalfHours));

  for refused in [0, -1_000, 1_500, 90_000] {
    assert_eq!(Rows::of_span(refused), None, "{refused}");
  }

  assert_eq!(
    read_readings(&Store::in_memory().unwrap(), None, "box", asked(1_500, None, None, None), 0)
      .unwrap_err()
      .to_string(),
    "a span is whole seconds under a minute or whole minutes from a minute up, not 1500 ms"
  );
}

fn asked(span_ms: i64, after_ms: Option<i64>, before_ms: Option<i64>, limit: Option<usize>) -> Asked {
  Asked {
    span_ms,
    after_ms,
    before_ms,
    limit,
  }
}

fn uses(page: &hardware::series::ReadingPage) -> Vec<(i64, f64, f64, f64)> {
  page
    .items
    .iter()
    .map(|reading| {
      let band = reading.cpu.use_pct.unwrap();

      (reading.at, band.min, band.max, band.mean)
    })
    .collect()
}

#[test]
fn the_live_record_s_seconds_are_paged_after_an_instant_and_gathered_into_wider_stretches() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();
  let store = Store::in_memory().unwrap();

  for (at, usage) in [(100, 0.1), (101, 0.3), (102, 0.5), (106, 0.9)] {
    writer.reading("box", at, &view(usage));
  }

  writer.reading("other", 101, &view(0.9));
  writer.written();

  let reader = Reader::open(&path).unwrap();
  let read = |asked: Asked| read_readings(&store, Some(&reader), "box", asked, 200).unwrap();
  let seconds = read(asked(1_000, None, None, None));

  assert_eq!(
    uses(&seconds),
    [
      (100_000, 10.0, 10.0, 10.0),
      (101_000, 30.0, 30.0, 30.0),
      (102_000, 50.0, 50.0, 50.0),
      (106_000, 90.0, 90.0, 90.0)
    ]
  );
  assert!(
    seconds
      .items
      .iter()
      .all(|reading| reading.span_ms == 1_000 && reading.machine_name == "box")
  );
  assert!(!seconds.more);

  // The next page starts after the last item's time, and a limit says whether more stand past it.
  let first = read(asked(1_000, Some(100_000), None, Some(2)));

  assert_eq!(uses(&first).iter().map(|one| one.0).collect::<Vec<_>>(), [101_000, 102_000]);
  assert!(first.more);
  assert_eq!(uses(&read(asked(1_000, Some(102_000), Some(106_000), None))), []);

  // Five seconds a stretch: a stretch nothing was recorded in is left out.
  assert_eq!(
    uses(&read(asked(5_000, None, None, None))),
    [(100_000, 10.0, 50.0, 30.0), (105_000, 90.0, 90.0, 90.0)]
  );
}

#[test]
fn a_store_that_never_recorded_reads_as_an_empty_page() {
  let store = Store::in_memory().unwrap();
  let page = read_readings(&store, None, "box", asked(1_800_000, None, None, None), 3_600).unwrap();

  assert!(page.items.is_empty() && !page.more);
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

  let page =
    read_readings(&store, None, "box", asked(1_800_000, None, Some(3_600_000), None), 3_600).unwrap();

  assert_eq!(uses(&page), [(0, 10.0, 30.0, 20.0), (1_800_000, 50.0, 50.0, 50.0)]);

  // The minutes' rows hold every minute, the rolled-up one among them.
  let minutes =
    read_readings(&store, None, "box", asked(60_000, None, Some(3_600_000), None), 3_600).unwrap();

  assert_eq!(uses(&minutes).iter().map(|one| one.0).collect::<Vec<_>>(), [60_000, 1_860_000]);
}

#[test]
fn a_reading_lays_its_figures_out_per_piece_of_hardware() {
  let band = |value: f64| Band {
    min: value,
    max: value,
    mean: value,
  };
  let figures: BTreeMap<String, Band> = [
    ("cpu|use", 12.0),
    ("cpu|temp", 61.3),
    ("memory|gb", 8.5),
    ("gpu1|use", 40.0),
    ("gpu0|watts", 120.0),
    ("gpu0:vram|gb", 3.25),
    ("storage:nvme0n1|gb", 400.0),
    ("storage:net:host:/share|gb", 10.0),
    ("storage:nvme0n1|temp", 41.0),
    ("fan:nct6798:fan2|rpm", 900.0),
    ("unknown|figure", 1.0),
  ]
  .into_iter()
  .map(|(key, value)| (key.to_owned(), band(value)))
  .collect();
  let reading = reading_of("box", 5_000, 1_000, &figures);

  assert_eq!(
    (reading.cpu.use_pct, reading.cpu.celsius, reading.cpu.watts),
    (Some(band(12.0)), Some(band(61.3)), None)
  );
  assert_eq!(reading.memory.used_gb, Some(band(8.5)));
  assert_eq!(reading.gpus.iter().map(|gpu| gpu.index).collect::<Vec<_>>(), [0, 1]);
  assert_eq!(
    (reading.gpus[0].watts, reading.gpus[0].vram_used_gb),
    (Some(band(120.0)), Some(band(3.25)))
  );
  assert_eq!(reading.gpus[1].use_pct, Some(band(40.0)));
  assert_eq!(
    reading
      .storage
      .iter()
      .map(|one| (one.key.as_str(), one.used_gb, one.celsius))
      .collect::<Vec<_>>(),
    [
      ("net:host:/share", Some(band(10.0)), None),
      ("nvme0n1", Some(band(400.0)), Some(band(41.0)))
    ]
  );
  assert_eq!(
    reading
      .fans
      .iter()
      .map(|fan| (fan.chip.as_str(), fan.label.as_str()))
      .collect::<Vec<_>>(),
    [("nct6798", "fan2")]
  );

  let live = live_reading("box", 7, &history_sample(&view(0.25)));

  assert_eq!((live.at, live.span_ms, live.cpu.use_pct), (7_000, 1_000, Some(band(25.0))));
}

fn view(usage: f64) -> hardware::view::HardwareView {
  hardware_view(&snapshot(usage))
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
