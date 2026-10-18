//! One machine's reading as a screen draws it: each piece of hardware with its own use, fill, temperature,
//! limit and power, and what the machine did not answer left unread.

use hardware::probe::{CpuThread, Fan, Gpu, Memory, Sensor, ServiceCount, Snapshot, Watts};
use hardware::storage::{StorageDevice, StorageVolume};
use hardware::view::hardware_view;
use serde_json::{Value, json};

fn sensor(chip: &str, device: Option<&str>, label: &str, celsius: f64, limit: Option<f64>) -> Sensor {
  Sensor {
    chip: chip.into(),
    device: device.map(str::to_owned),
    label: label.into(),
    celsius,
    limit_celsius: limit,
  }
}

fn thread(cpu: f64, mhz: f64) -> CpuThread {
  CpuThread {
    cpu,
    package: Some(0.0),
    core: Some(cpu),
    l3: Some("0-1".into()),
    l3_size: Some("32768K".into()),
    usage: Some(0.3),
    mhz: Some(mhz),
  }
}

fn volume(mount: &str, total_kb: f64, used_kb: f64) -> StorageVolume {
  StorageVolume {
    mount: mount.into(),
    fs_type: Some("ext4".into()),
    total_kb,
    used_kb,
    available_kb: total_kb - used_kb,
  }
}

fn snapshot() -> Snapshot {
  Snapshot {
    host: Some("box".into()),
    kernel: Some("6.6".into()),
    wsl: false,
    os: Some("Ubuntu 24.04.4 LTS".into()),
    host_os: None,
    sensor_library: None,
    uptime_seconds: Some(100.0),
    load: Some([1.0, 2.0, 3.0]),
    memory: Some(Memory {
      total_kb: 1000.0,
      available_kb: 250.0,
      swap_total_kb: 100.0,
      swap_free_kb: 75.0,
    }),
    cpu_model: Some("AMD Ryzen 7 5800X 8-Core Processor".into()),
    cpu_usage: Some(0.3),
    clock_source: Some("cpufreq"),
    threads: vec![thread(0.0, 4500.0), thread(1.0, 3500.0)],
    cpu_watts: Some(Watts::Read(40.0)),
    sensors: vec![
      sensor("coretemp", None, "Package id 0", 70.0, Some(100.0)),
      sensor("nvme", Some("nvme0"), "Composite", 45.0, None),
      sensor("nct6797", None, "SYSTIN", 41.0, None),
    ],
    fans: vec![Fan {
      chip: "nct6797".into(),
      label: "fan2".into(),
      rpm: 800.0,
    }],
    gpus: vec![Gpu {
      index: 0.0,
      name: "GPU".into(),
      usage: Some(0.5),
      celsius: Some(80.0),
      mhz: Some(1.0),
      max_mhz: Some(2.0),
      memory_mhz: Some(3.0),
      watts: Some(100.0),
      watt_limit: Some(200.0),
      memory_used_mb: Some(1.0),
      memory_total_mb: Some(2.0),
      fan_percent: None,
      limit_celsius: Some(93.0),
      throttling: Some(false),
    }],
    storage: vec![StorageDevice {
      key: "nvme0n1".into(),
      name: "SSD".into(),
      kind: Some("nvme"),
      size_bytes: Some(1e12),
      celsius: Some(45.0),
      limit_celsius: Some(85.0),
      volumes: vec![volume("/", 100.0, 90.0), volume("/boot", 10.0, 1.0)],
    }],
    services: (
      Some(ServiceCount {
        running: 10.0,
        failed: vec!["a.service".into()],
      }),
      Some(ServiceCount {
        running: 2.0,
        failed: vec!["b.service".into()],
      }),
    ),
  }
}

/// Figures compared as numbers, whatever form the JSON wrote each in.
fn numbers(values: &[&Value]) -> Vec<Value> {
  values
    .iter()
    .map(|one| one.as_f64().map_or((*one).clone(), |number| json!(number)))
    .collect()
}

fn view(snapshot: &Snapshot) -> Value {
  serde_json::to_value(hardware_view(snapshot)).unwrap()
}

fn labels(list: &Value) -> Vec<Value> {
  list.as_array().unwrap().iter().map(|one| one["label"].clone()).collect()
}

#[test]
fn gives_each_piece_of_hardware_its_own_use_fill_temperature_and_limit_and_power() {
  let view = view(&snapshot());
  let cpu = &view["cpu"];

  assert_eq!(
    numbers(&[
      &cpu["usage"],
      &cpu["celsius"],
      &cpu["limitCelsius"],
      &cpu["mhz"],
      &cpu["peakMhz"],
      &cpu["watts"],
      &cpu["threadCount"]
    ]),
    numbers(&[&json!(0.3), &json!(70), &json!(100), &json!(4000), &json!(4500), &json!(40), &json!(2)])
  );
  assert_eq!(labels(&cpu["sensors"]), [json!("Package id 0")]);
  assert_eq!(cpu["chip"]["rows"][0][0]["label"], "CCD0");

  let memory = &view["memory"];

  assert_eq!(
    numbers(&[
      &memory["usedKb"],
      &memory["totalKb"],
      &memory["share"],
      &memory["swapUsedKb"],
      &memory["swapShare"]
    ]),
    numbers(&[&json!(750), &json!(1000), &json!(0.75), &json!(25), &json!(0.25)])
  );

  let gpu = &view["gpus"][0];

  assert_eq!(
    numbers(&[
      &gpu["celsius"],
      &gpu["limitCelsius"],
      &gpu["throttling"],
      &gpu["wattShare"],
      &gpu["memoryShare"]
    ]),
    numbers(&[&json!(80), &json!(93), &json!(false), &json!(0.5), &json!(0.5)])
  );

  // The drive keeps its own temperature, its limit and its volumes; the board keeps what is left.
  let drive = &view["storage"][0];

  assert_eq!(
    numbers(&[&drive["celsius"], &drive["limitCelsius"], &drive["usedKb"], &drive["totalKb"]]),
    numbers(&[&json!(45), &json!(85), &json!(91), &json!(110)])
  );
  assert_eq!(
    drive["volumes"]
      .as_array()
      .unwrap()
      .iter()
      .map(|one| (one["mount"].clone(), one["share"].clone()))
      .collect::<Vec<_>>(),
    [(json!("/"), json!(0.9)), (json!("/boot"), json!(0.1))]
  );
  assert_eq!(labels(&view["board"]["sensors"]), [json!("SYSTIN")]);
  assert_eq!(numbers(&[&view["services"]["running"]]), numbers(&[&json!(12)]));
  assert_eq!(view["services"]["failed"], json!(["a.service", "b.service (user)"]));
  assert_eq!(view["os"], "Ubuntu 24.04.4 LTS");
}

#[test]
fn what_the_machine_did_not_answer_stays_unread() {
  let mut unread = snapshot();

  unread.memory = None;
  unread.sensors = vec![];
  unread.gpus = vec![];
  unread.storage = vec![];
  unread.cpu_watts = Some(Watts::Denied);
  unread.threads = vec![];
  unread.services = (None, None);

  let view = view(&unread);

  assert_eq!(
    [&view["memory"]["usedKb"], &view["memory"]["share"], &view["memory"]["swapTotalKb"]],
    [&Value::Null, &Value::Null, &Value::Null]
  );
  assert_eq!(
    [
      &view["cpu"]["celsius"],
      &view["cpu"]["limitCelsius"],
      &view["cpu"]["mhz"],
      &view["cpu"]["watts"]
    ],
    [&Value::Null, &Value::Null, &Value::Null, &json!("denied")]
  );
  assert_eq!(view["services"], json!({ "running": null, "failed": null }));
}
