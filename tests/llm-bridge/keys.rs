//! The keys the door takes: a key read only from a private file holding one, the list read whole, and a
//! bearer matched whole and in constant time.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use llm_bridge::keys::{bearer_matches, read_door_keys, read_service_key};

fn key_file(dir: &Path, name: &str, mode: u32, text: &str) -> String {
  let path = dir.join(name);
  std::fs::write(&path, format!("{text}\n")).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
  path.display().to_string()
}

#[test]
fn a_key_is_read_only_from_a_private_file_that_holds_one() {
  let dir = tempfile::tempdir().unwrap();
  let key = "a".repeat(64);
  assert_eq!(read_service_key(Path::new(&key_file(dir.path(), "one", 0o600, &key))), Some(key));
  assert_eq!(read_service_key(Path::new(&key_file(dir.path(), "loose", 0o644, "b"))), None);
  assert_eq!(read_service_key(Path::new(&key_file(dir.path(), "empty", 0o600, "  "))), None);
  assert_eq!(read_service_key(Path::new("/nonexistent/echo.key")), None);
}

#[test]
fn the_door_takes_any_key_its_list_names_and_none_where_the_list_is_missing() {
  let dir = tempfile::tempdir().unwrap();
  let (key, other) = ("a".repeat(64), "c".repeat(64));
  let list = dir.path().join("tunnel-door.keys");
  let listed = [
    key_file(dir.path(), "one", 0o600, &key),
    "/nonexistent/delta.key".to_owned(),
    String::new(),
    key_file(dir.path(), "two", 0o600, &other),
  ];
  std::fs::write(&list, listed.join("\n")).unwrap();
  assert_eq!(read_door_keys(&list), [key, other]);
  assert!(read_door_keys(Path::new("/nonexistent/tunnel-door.keys")).is_empty());
}

#[test]
fn a_bearer_is_matched_whole() {
  let key = "a".repeat(64);
  let bearer = |text: &str| bearer_matches(Some(text.as_bytes()), &key);
  assert!(bearer(&format!("Bearer {key}")));
  assert!(!bearer(&format!("Bearer {}", &key[1..])));
  assert!(!bearer(&format!("Bearer {key}a")));
  assert!(!bearer(&key));
  assert!(!bearer_matches(None, &key));
}

// The median time of matching a bearer of the key's length against the key, many times over.
fn median_match(bearer: &[u8], key: &str) -> Duration {
  let mut times: Vec<Duration> = (0..15)
    .map(|_| {
      let started = Instant::now();
      std::hint::black_box(bearer_matches(Some(std::hint::black_box(bearer)), key));
      started.elapsed()
    })
    .collect();
  times.sort();
  times[times.len() / 2]
}

#[test]
fn a_bearer_wrong_from_its_first_byte_takes_as_long_as_one_wrong_only_at_its_last() {
  // A long key, so a comparison that stops at the first difference would be done at once on one bearer
  // and walk the whole key on the other.
  let key = "k".repeat(256 * 1024);
  let mut early = format!("Bearer {key}").into_bytes();
  let mut late = early.clone();
  early["Bearer ".len()] = b'x';
  *late.last_mut().unwrap() = b'x';
  let (early, late) = (median_match(&early, &key), median_match(&late, &key));
  let ratio = early.as_secs_f64() / late.as_secs_f64();
  assert!(
    (0.5..2.0).contains(&ratio),
    "a first-byte miss took {early:?} and a last-byte miss {late:?}"
  );
}
