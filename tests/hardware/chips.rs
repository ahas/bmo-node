//! The chip as it physically stands: the kernel's cache size read the way a person does, the sensors placed
//! on the package, the dies and the cores, and each family's layout.

use hardware::chips::{cache_size, chip_view};
use hardware::probe::{CpuThread, Sensor};
use serde_json::{Value, json};

/// Cores of two threads, numbered the way Linux numbers an SMT machine: the first thread of every core,
/// then the second.
fn threads(cores: usize, l3: impl Fn(usize) -> Option<&'static str>) -> Vec<CpuThread> {
  (0..cores * 2)
    .map(|cpu| CpuThread {
      cpu: cpu as f64,
      package: Some(0.0),
      core: Some((cpu % cores) as f64),
      l3: l3(cpu % cores).map(str::to_owned),
      l3_size: Some("16384K".into()),
      usage: Some(if cpu % cores == 0 { 0.9 } else { 0.1 }),
      mhz: Some(4000.0 + (cpu % cores) as f64),
    })
    .collect()
}

fn sensor(chip: &str, label: &str, celsius: f64) -> Sensor {
  Sensor {
    chip: chip.into(),
    device: None,
    label: label.into(),
    celsius,
    limit_celsius: None,
  }
}

fn chip(model: &str, threads: &[CpuThread], sensors: &[Sensor]) -> Value {
  serde_json::to_value(chip_view(Some(model), threads, sensors).expect("a chip")).unwrap()
}

fn clusters(die: &Value) -> Vec<Value> {
  die["items"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|one| one["kind"] == "cluster")
    .cloned()
    .collect()
}

fn labels(cores: &Value) -> Vec<String> {
  cores
    .as_array()
    .unwrap()
    .iter()
    .map(|core| core["label"].as_str().unwrap().to_owned())
    .collect()
}

#[test]
fn reads_the_kernel_s_cache_size_the_way_a_person_does() {
  assert_eq!(cache_size(Some("16384K")).as_deref(), Some("16 MB"));
  assert_eq!(cache_size(Some("512K")).as_deref(), Some("512 KB"));
  assert_eq!(cache_size(Some("32M")).as_deref(), Some("32 MB"));
  assert_eq!(cache_size(None), None);
}

#[test]
fn a_zen_2_desktop_part_is_one_ccd_beside_an_empty_place_above_the_io_die_two_ccxs_of_four() {
  let chip = chip(
    "AMD Ryzen 7 3700X 8-Core Processor",
    &threads(8, |core| Some(if core < 4 { "0-3,8-11" } else { "4-7,12-15" })),
    &[
      sensor("k10temp", "Tctl", 62.0),
      sensor("k10temp", "Tccd1", 57.0),
      sensor("nvme", "Composite", 44.0),
    ],
  );

  assert_eq!(chip["celsius"], 62.0);
  assert_eq!(chip["rows"][0][1], Value::Null);
  assert_eq!(chip["rows"][1][0]["role"], "io");

  let ccd = &chip["rows"][0][0];

  assert_eq!(
    (ccd["label"].clone(), ccd["celsius"].clone(), ccd["direction"].clone()),
    (json!("CCD0"), json!(57.0), json!("column"))
  );
  assert_eq!(labels(&ccd["items"]), ["CCX0", "Infinity Fabric", "CCX1"]);
  assert_eq!(
    clusters(ccd)
      .iter()
      .map(|one| (labels(&one["left"]), labels(&one["right"]), one["cache"].clone()))
      .collect::<Vec<_>>(),
    [
      (
        vec!["C0".to_owned(), "C1".into()],
        vec!["C2".to_owned(), "C3".into()],
        json!("L3 16 MB")
      ),
      (
        vec!["C4".to_owned(), "C5".into()],
        vec!["C6".to_owned(), "C7".into()],
        json!("L3 16 MB")
      ),
    ]
  );
}

#[test]
fn a_second_ccd_s_sensor_stands_on_the_second_ccd() {
  let chip = chip(
    "AMD Ryzen 9 3950X 16-Core Processor",
    &threads(16, |_| None),
    &[sensor("k10temp", "Tccd1", 57.0), sensor("k10temp", "Tccd2", 55.0)],
  );

  assert_eq!(
    (chip["rows"][0][0]["celsius"].clone(), chip["rows"][0][1]["celsius"].clone()),
    (json!(57.0), json!(55.0))
  );
}

#[test]
fn a_core_s_figures_are_its_threads_the_mean_of_their_use_and_the_fastest_clock() {
  let chip = chip("AMD Ryzen 7 3700X 8-Core Processor", &threads(8, |_| None), &[]);
  let core = &clusters(&chip["rows"][0][0])[0]["left"][0];

  assert_eq!(core["label"], "C0");
  assert_eq!(core["usage"], 0.9);
  assert_eq!(core["mhz"], 4000.0);
  assert_eq!(core["threads"], json!([{ "cpu": 0.0, "usage": 0.9 }, { "cpu": 8.0, "usage": 0.9 }]));
}

#[test]
fn a_sixteen_core_zen_2_part_fills_both_ccd_places() {
  let chip = chip("AMD Ryzen 9 3950X 16-Core Processor", &threads(16, |_| None), &[]);

  assert_eq!(
    (chip["rows"][0][0]["label"].clone(), chip["rows"][0][1]["label"].clone()),
    (json!("CCD0"), json!("CCD1"))
  );

  let second: Vec<String> = clusters(&chip["rows"][0][1])
    .iter()
    .flat_map(|one| [labels(&one["left"]), labels(&one["right"])].concat())
    .collect();

  assert_eq!(second, ["C8", "C9", "C10", "C11", "C12", "C13", "C14", "C15"]);
}

#[test]
fn a_zen_3_desktop_part_is_one_ccx_of_eight_four_either_side_of_the_l3() {
  let chip = chip("AMD Ryzen 7 5800X 8-Core Processor", &threads(8, |_| None), &[]);
  let ccd = &chip["rows"][0][0];
  let ccx = &clusters(ccd)[0];

  assert_eq!(ccd["items"].as_array().unwrap().len(), 1);
  assert_eq!(labels(&ccx["left"]), ["C0", "C1", "C2", "C3"]);
  assert_eq!(labels(&ccx["right"]), ["C4", "C5", "C6", "C7"]);
}

#[test]
fn an_apu_is_not_drawn_as_a_desktop_chiplet_part() {
  let chip = chip("AMD Ryzen 7 5700G with Radeon Graphics", &threads(8, |_| None), &[]);

  assert_eq!(chip["rows"].as_array().unwrap().len(), 1);
  assert_eq!(chip["rows"][0][0]["label"], "Die");
}

#[test]
fn an_intel_ring_part_is_the_gpu_the_cores_beside_the_llc_and_the_system_agent_the_gpu_off_on_an_f_part() {
  let chip = chip(
    "Intel(R) Core(TM) i9-9900KF CPU @ 3.60GHz",
    &threads(8, |_| None),
    &[sensor("coretemp", "Core 2", 71.0)],
  );
  let die = &chip["rows"][0][0];

  assert_eq!(die["direction"], "row");
  assert_eq!(labels(&die["items"]), ["GPU", "Ring", "System Agent"]);
  assert_eq!(
    (die["items"][0]["kind"].clone(), die["items"][0]["off"].clone()),
    (json!("part"), json!(true))
  );

  let ring = &clusters(die)[0];

  assert_eq!(ring["left"].as_array().unwrap().len(), 8);
  assert_eq!(ring["right"].as_array().unwrap().len(), 0);
  assert_eq!(ring["left"][2]["celsius"], 71.0);
  assert_eq!(
    chip_k("Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz")["rows"][0][0]["items"][0]["off"],
    false
  );
}

fn chip_k(model: &str) -> Value {
  chip(model, &threads(6, |_| None), &[])
}

#[test]
fn a_hybrid_intel_part_and_an_unknown_one_fall_to_one_die_grouped_by_the_l3() {
  assert_eq!(
    chip("12th Gen Intel(R) Core(TM) i7-12700K", &threads(4, |_| None), &[])["rows"][0][0]["label"],
    "Die"
  );

  let chip = chip("Some Vendor CPU", &threads(4, |core| Some(if core < 2 { "a" } else { "b" })), &[]);

  assert_eq!(
    clusters(&chip["rows"][0][0]).iter().map(|one| one["label"].clone()).collect::<Vec<_>>(),
    [json!("L3 0"), json!("L3 1")]
  );
}

#[test]
fn a_machine_whose_threads_could_not_be_read_has_no_chip() {
  assert!(chip_view(Some("AMD Ryzen 7 3700X 8-Core Processor"), &[], &[]).is_none());
}
