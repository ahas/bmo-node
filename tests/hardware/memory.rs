//! Who holds a machine's memory: a look read out of a live run, the minute's worst kept in the store, and a
//! machine's samples read back from the record and the store.

use std::sync::Arc;

use hardware::memory::{self, AskedSamples, KEEP_SECONDS, holders, read_samples, sample_of};
use hardware::probe::{LiveProbe, sections};
use hardware::record::{self, Reader, Writer};
use hardware::view::{HardwareView, hardware_view};
use store::Store;

// As a node's run prints it: the first process a unit's, the third with none and a name holding a space.
const LOOK: &str = "@@procmem
43314068 223448 2226456 798408 engine.service engine-host
85508 12 1877444 2308187 bmo-voice-server.service python
2048 0 0 42 - tmux: server
not a process
@@unitmem
53688296 40548688 4661536 9173252 3630972 engine.service
15536076 15252 13148328 13168248 2255772 bmo-voice-server.service
not a unit
";

const MEMINFO: &str = "@@meminfo
MemTotal:       65750584 kB
MemAvailable:    4742372 kB
SwapTotal:       8388604 kB
SwapFree:        2238540 kB
Shmem:          17809900 kB
";

fn view() -> HardwareView {
  let text = std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hardware/fixtures/linux.txt"),
  )
  .unwrap();
  let mut found = sections(&text);

  found.extend(sections(MEMINFO));

  hardware_view(&hardware::probe::read_sections(&found).unwrap())
}

fn sample(at: i64, available_kb: f64) -> memory::MemorySample {
  let mut read = view();

  read.memory.used_kb = read.memory.total_kb.map(|total| total - available_kb);

  sample_of("n4", at, &read, vec![], vec![])
}

fn text(at: i64, available_kb: f64) -> (i64, String) {
  (at, serde_json::to_string(&sample(at, available_kb)).unwrap())
}

#[test]
fn a_look_names_each_process_and_each_unit_with_what_the_unit_is_charged_and_no_process_shows() {
  let (processes, units) = holders(&sections(LOOK)).unwrap();

  assert_eq!(
    processes
      .iter()
      .map(|one| (one.pid, one.name.as_str(), one.unit.as_deref()))
      .collect::<Vec<_>>(),
    [
      (798408, "engine-host", Some("engine.service")),
      (2308187, "python", Some("bmo-voice-server.service")),
      (42, "tmux: server", None),
    ]
  );

  let voice = &processes[1];

  assert_eq!((voice.rss_mb, voice.shmem_mb, voice.swap_mb), (84, 0, 1833));

  let unit = &units[1];

  assert_eq!(units.len(), 2);
  assert_eq!(unit.unit, "bmo-voice-server.service");
  assert_eq!((unit.anon_mb, unit.shmem_mb, unit.file_mb, unit.swap_mb), (15, 12840, 12860, 2203));
  assert_eq!(unit.current_mb, 15172 - 2203);
}

#[test]
fn a_reading_that_took_no_look_carries_none() {
  assert!(holders(&sections(MEMINFO)).is_none());
}

#[test]
fn a_sample_carries_the_machines_figures_at_that_reading() {
  let (processes, units) = holders(&sections(LOOK)).unwrap();
  let sample = sample_of("n4", 100, &view(), processes, units);

  assert_eq!(sample.machine_name, "n4");
  assert_eq!(sample.at, 100_000);
  assert_eq!(sample.available_gb, Some(4.52));
  assert_eq!(sample.shmem_gb, Some(16.98));
  assert_eq!(sample.swap_used_gb, Some(5.87));
  assert_eq!(sample.units.len(), 2);
}

#[test]
fn a_reading_s_figures_say_what_stood_available_shared_and_swapped() {
  let figures: serde_json::Map<String, serde_json::Value> = hardware::view::history_sample(&view());
  let reading = hardware::series::live_reading("n4", 100, &figures);
  let mean = |band: Option<hardware::series::Band>| band.map(|band| band.mean);

  assert_eq!(mean(reading.memory.available_gb), Some(4.52));
  assert_eq!(mean(reading.memory.shmem_gb), Some(16.98));
  assert_eq!(mean(reading.memory.swap_used_gb), Some(5.87));
  assert_eq!(mean(reading.memory.used_gb), Some(58.18));
}

#[test]
fn a_live_run_s_look_stands_for_its_own_reading_alone() {
  let mut probe = LiveProbe::default();
  let reading = |uptime: i64, look: &str| {
    format!(
      "@@host\nbox\n@@uptime\n{uptime}\n@@stat\ncpu  1 0 0 {uptime}00 0 0 0 0\ncpu0 1 0 0 {uptime}00 0 0 0 0\n{look}@@end\n"
    )
  };

  probe.take(&reading(10, ""));

  assert!(holders(&probe.take(&reading(11, LOOK)).unwrap()).is_some());
  assert!(holders(&probe.take(&reading(12, "")).unwrap()).is_none());
}

#[test]
fn each_closed_minute_keeps_its_worst_look_under_its_own_second_once() {
  let store = Store::in_memory().unwrap();
  let looks = vec![
    text(60_005, 9_000_000.0),
    text(60_010, 1_000_000.0),
    text(60_015, 5_000_000.0),
    text(60_065, 7_000_000.0),
    // The minute still open.
    text(60_125, 100.0),
  ];

  assert_eq!(memory::fold(&store, "n4", 60_130, 900, looks.clone()).unwrap(), 2);

  let kept = store.memory_samples("n4", 0, i64::MAX).unwrap();

  assert_eq!(kept.iter().map(|row| row.at).collect::<Vec<_>>(), [60_010, 60_065]);

  // A pass over the same looks writes nothing again.
  assert_eq!(memory::fold(&store, "n4", 60_130, 900, looks).unwrap(), 0);
  // The next minute's closing takes that minute alone.
  assert_eq!(memory::fold(&store, "n4", 60_190, 900, vec![text(60_125, 100.0)]).unwrap(), 1);
}

#[test]
fn a_sample_is_kept_seven_days() {
  let store = Store::in_memory().unwrap();
  let now = 10 * 86_400;

  store
    .put_memory_samples(
      "n4",
      &[text(now - KEEP_SECONDS - 1, 1.0), text(now - KEEP_SECONDS, 1.0), text(now - 60, 1.0)],
    )
    .unwrap();

  assert_eq!(memory::prune(&store, now).unwrap(), 1);
  assert_eq!(store.memory_samples("n4", 0, i64::MAX).unwrap().len(), 2);
}

#[test]
fn samples_are_read_from_the_store_before_the_record_s_reach_and_from_the_record_within_it() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();
  let store = Arc::new(Store::in_memory().unwrap());
  let now = 10 * 86_400;
  let within = now - 100;
  let before_reach = now - record::KEEP_SECONDS - 60;

  // The minute's row of a sample the record still holds is not read a second time.
  store
    .put_memory_samples(
      "n4",
      &[text(now - KEEP_SECONDS - 60, 1.0), text(before_reach, 2.0), text(within, 3.0)],
    )
    .unwrap();
  writer.memory("n4", within, &sample(within, 3.0));
  writer.memory("n4", within + 5, &sample(within + 5, 4.0));
  writer.memory("other", within, &sample(within, 5.0));
  writer.written();

  let reader = Reader::open(&path);
  let read = |asked| read_samples(&store, reader.as_ref(), "n4", asked, now).unwrap();
  let seconds =
    |page: &memory::MemorySamplePage| page.items.iter().map(|one| one.at / 1000).collect::<Vec<_>>();

  assert_eq!(seconds(&read(AskedSamples::default())), [before_reach, within, within + 5]);

  let first = read(AskedSamples {
    limit: Some(2),
    ..AskedSamples::default()
  });

  assert_eq!(seconds(&first), [before_reach, within]);
  assert!(first.more);

  let next = read(AskedSamples {
    after_ms: Some(first.items[1].at),
    ..AskedSamples::default()
  });

  assert_eq!(seconds(&next), [within + 5]);
  assert!(!next.more);
  assert_eq!(
    seconds(&read(AskedSamples {
      before_ms: Some(within * 1000),
      ..AskedSamples::default()
    })),
    [before_reach]
  );
}

#[test]
fn the_record_lets_a_look_go_past_its_reach() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();

  writer.memory("n4", 100, &sample(100, 1.0));
  writer.memory("n4", 2000, &sample(2000, 1.0));
  writer.reading("n4", 2000, &view());
  writer.prune(2000, record::KEEP_SECONDS);
  writer.written();

  let reader = Reader::open(&path).unwrap();

  assert_eq!(
    reader
      .memory_samples("n4", 0, 3000)
      .unwrap()
      .iter()
      .map(|one| one.0)
      .collect::<Vec<_>>(),
    [2000]
  );
}
