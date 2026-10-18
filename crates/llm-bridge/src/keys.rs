//! The keys the door takes: each service's key file named in the door's list, read afresh for every
//! connection, and a bearer matched against them in constant time.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use subtle::ConstantTimeEq;

/// A service's key as the door reads it: a file only its owner may read or write, holding something;
/// anything else is no key at all.
pub fn read_service_key(path: &Path) -> Option<String> {
  let mode = std::fs::metadata(path).ok()?.permissions().mode();
  if mode & 0o777 != 0o600 {
    return None;
  }
  let key = std::fs::read_to_string(path).ok()?.trim().to_owned();
  (!key.is_empty()).then_some(key)
}

/// The keys of every service the door's list names, one key file's path to a line; a list that is
/// missing names none.
pub fn read_door_keys(list: &Path) -> Vec<String> {
  let Ok(listed) = std::fs::read_to_string(list) else {
    return Vec::new();
  };
  listed
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .filter_map(|line| read_service_key(Path::new(line)))
    .collect()
}

/// Whether a request's `authorization` is `Bearer <key>`, its key compared in constant time: how long
/// the comparison takes says nothing of how much of the key a caller had right.
pub fn bearer_matches(header: Option<&[u8]>, key: &str) -> bool {
  let Some(given) = header.and_then(|header| header.strip_prefix(b"Bearer ")) else {
    return false;
  };
  given.ct_eq(key.as_bytes()).into()
}
