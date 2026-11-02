//! The parser held to the readings written beside each probe output: every output under the fixtures read
//! as the recorder reads it, against the readings stored with it.

use hardware::probe::{LiveProbe, Sections, read_sections, sections};
use hardware::view::{hardware_view, history_sample};
use serde_json::{Value, json};

use crate::hardware::fixtures;

fn reading(found: &Sections) -> Value {
  let view = hardware_view(&read_sections(found).expect("a whole reading"));

  json!({ "view": view, "sample": history_sample(&view) })
}

/// A fixture read as the recorder reads it: a live run's output a reading at a time as each closes, any
/// other as the probe run once.
fn readings(name: &str, text: &str) -> Value {
  if !name.starts_with("live-") {
    return json!([reading(&sections(text))]);
  }

  let mut probe = LiveProbe::default();
  let mut readings = Vec::new();
  let mut lines: Vec<&str> = Vec::new();

  for line in text.split('\n') {
    if line.trim() != "@@end" {
      lines.push(line);
      continue;
    }

    let whole = probe.take(&format!("{}\n@@end\n", lines.join("\n")));

    lines.clear();
    readings.push(whole.as_ref().map_or(Value::Null, reading));
  }

  Value::Array(readings)
}

/// Where two readings differ, by the path to the first difference; numbers compared as numbers.
fn differs(path: &str, mine: &Value, theirs: &Value) -> Option<String> {
  match (mine, theirs) {
    (Value::Number(a), Value::Number(b)) => {
      let (a, b) = (a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN));

      ((a - b).abs() > 1e-9 * a.abs().max(1.0)).then(|| format!("{path}: {a} against {b}"))
    }
    (Value::Array(a), Value::Array(b)) => {
      if a.len() != b.len() {
        return Some(format!("{path}: {} items against {}", a.len(), b.len()));
      }

      a.iter()
        .zip(b)
        .enumerate()
        .find_map(|(at, (a, b))| differs(&format!("{path}[{at}]"), a, b))
    }
    (Value::Object(a), Value::Object(b)) => {
      let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();

      keys.sort();
      keys.dedup();
      keys.into_iter().find_map(|key| {
        differs(
          &format!("{path}.{key}"),
          a.get(key).unwrap_or(&Value::Null),
          b.get(key).unwrap_or(&Value::Null),
        )
        .or_else(|| {
          (a.contains_key(key) != b.contains_key(key))
            .then(|| format!("{path}.{key}: present on one side only"))
        })
      })
    }
    _ => (mine != theirs).then(|| format!("{path}: {mine} against {theirs}")),
  }
}

#[test]
fn every_probe_output_reads_as_the_readings_written_beside_it() {
  let mut names: Vec<String> = std::fs::read_dir(fixtures())
    .expect("the fixtures")
    .flatten()
    .map(|entry| entry.file_name().to_string_lossy().into_owned())
    .filter_map(|name| name.strip_suffix(".txt").map(str::to_owned))
    .collect();

  names.sort();
  assert_eq!(names.len(), 16, "the sixteen outputs stand");

  let failed: Vec<String> = names
    .iter()
    .filter_map(|name| {
      let text = std::fs::read_to_string(fixtures().join(format!("{name}.txt"))).expect("the output");
      let written: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join(format!("{name}.json"))).expect("the readings"),
      )
      .expect("readings that read");

      differs(name, &readings(name, &text), &written)
    })
    .collect();

  assert!(failed.is_empty(), "{}", failed.join("\n"));
}
