//! Secrets handed by name (docs/tools.md, 「Secrets handed by name」): the saved secrets a call names, read from
//! the caller's own space on this machine and handed to the command as variables, as files on the node, or both,
//! and every value handed masked in what the answer says.

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

use crate::caller::Space;

/// The most a secret's file holds: what a variable carries on every node's shell once it is base64-encoded.
pub const MOST_BYTES: u64 = 16 * 1024;

/// The fewest characters a value is masked at; a shorter one would mask ordinary output.
const FEWEST_MASKED: usize = 4;

/// How a secret is handed: as variables, as a file on the node, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
  Env,
  File,
  Both,
}

impl Way {
  fn env(self) -> bool {
    matches!(self, Self::Env | Self::Both)
  }

  fn file(self) -> bool {
    matches!(self, Self::File | Self::Both)
  }
}

/// One secret a call names: by its name alone, or with how it is handed.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Named {
  Name(String),
  Asked(Asked),
}

/// A secret named with how it is handed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asked {
  pub name: String,
  #[serde(default)]
  pub prefix: Option<String>,
  #[serde(default, rename = "as")]
  pub way: Option<Way>,
}

impl Named {
  fn asked(&self) -> Asked {
    match self {
      Self::Name(name) => Asked {
        name: name.clone(),
        prefix: None,
        way: None,
      },
      Self::Asked(asked) => asked.clone(),
    }
  }
}

/// One secret read: its name, its file's bytes, the fields they hold where they hold fields, and how it is
/// handed.
pub struct Secret {
  name: String,
  bytes: Vec<u8>,
  fields: Option<Vec<(String, String)>>,
  prefix: String,
  way: Way,
}

/// Whether `name` is a plain file name of a secret: no folder in it, no leading dot, and not the name the
/// secret store keeps a replaced value under.
fn plain(name: &str) -> bool {
  !name.is_empty()
    && !name.contains(['/', '\\', '\0'])
    && !name.starts_with('.')
    && !name.ends_with(".previous")
}

/// Whether `part` may stand at the head of a variable's name: letters, digits and underscores, not opening on a
/// digit.
fn variable_head(part: &str) -> bool {
  part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    && !part.bytes().next().is_some_and(|byte| byte.is_ascii_digit())
}

/// Whether `key` is a field's key as the console writes one: a capital letter or an underscore, then capitals,
/// digits and underscores.
fn field_key(key: &str) -> bool {
  let mut bytes = key.bytes();

  bytes.next().is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_')
    && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The fields a file holds, where every line of it is a `KEY=VALUE` line and no key comes twice.
///
/// This reads the console's own shape of a secret of several fields (bmo-web's `docs/accounts.md`, 「Secret
/// kinds」), not a dotenv file: the console writes each value as typed, unquoted, so a dotenv reader's quoting
/// and `$` expansion would change a value that holds a quote, a `#` or a `$`. A value is the line after its first
/// `=`, exactly.
fn fields(bytes: &[u8]) -> Option<Vec<(String, String)>> {
  let text = std::str::from_utf8(bytes).ok()?;
  let mut fields: Vec<(String, String)> = Vec::new();

  for line in text.lines().filter(|line| !line.is_empty()) {
    let (key, value) = line.split_once('=')?;

    if !field_key(key) || fields.iter().any(|(known, _)| known == key) {
      return None;
    }

    fields.push((key.to_owned(), value.to_owned()));
  }

  (!fields.is_empty()).then_some(fields)
}

/// The stem a secret's own variables are named by: its name in capitals, every character other than a letter or
/// a digit an underscore.
pub fn stem(name: &str) -> String {
  name
    .chars()
    .map(|one| if one.is_ascii_alphanumeric() { one.to_ascii_uppercase() } else { '_' })
    .collect()
}

/// Reads one secret out of `space`, or the reason it cannot be had, naming it and never what its file holds.
fn read_one(named: &Named, space: &Space) -> Result<Secret, String> {
  let Asked { name, prefix, way } = named.asked();

  if !plain(&name) {
    return Err(format!(
      "The secret name {name:?} is not a plain file name: no slash, no leading dot and no .previous ending."
    ));
  }

  let prefix = prefix.unwrap_or_default();

  if !variable_head(&prefix) {
    return Err(format!(
      "The prefix {prefix:?} of the secret {name} is not letters, digits and underscores opening on no digit."
    ));
  }

  let path = space.folder.join(&name);
  let missing = || format!("No secret named {name} stands among {}.", space.whose);
  let stands = std::fs::symlink_metadata(&path).map_err(|_| missing())?;

  if !stands.is_file() {
    return Err(missing());
  }

  if stands.len() > MOST_BYTES {
    return Err(format!("The secret {name} is larger than {} KiB, the most handed.", MOST_BYTES / 1024));
  }

  let bytes = read_file(&path).map_err(|kind| format!("The secret {name} cannot be read: {kind}."))?;
  let fields = fields(&bytes);
  let way = way.unwrap_or(if fields.is_some() { Way::Env } else { Way::File });

  Ok(Secret {
    name,
    bytes,
    fields,
    prefix,
    way,
  })
}

fn read_file(path: &Path) -> Result<Vec<u8>, std::io::ErrorKind> {
  std::fs::read(path).map_err(|why| why.kind())
}

/// Reads every secret `named` asks for out of `space`, or the reason the first that cannot be had cannot.
pub fn read(named: &[Named], space: &Space) -> Result<Vec<Secret>, String> {
  named.iter().map(|one| read_one(one, space)).collect()
}

/// A secret's file handed on the node: the variable carrying its bytes, the variable its path is set in, and its
/// file's name in the call's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
  pub carrier: String,
  pub path: String,
  pub file: String,
}

/// What a call's secrets hand the command: the variables laid over the call's own, the files written on the
/// node, and the values masked in the answer with the name of the secret each is.
#[derive(Debug, Default)]
pub struct Handing {
  pub env: BTreeMap<String, String>,
  pub files: Vec<Carried>,
  masks: Vec<(String, String)>,
}

impl Handing {
  /// The handing of `secrets` beside the call's own variables `given`, or the reason it cannot be, naming a
  /// variable that would be set twice.
  pub fn of(secrets: &[Secret], given: &BTreeMap<String, String>) -> Result<Self, String> {
    let mut handing = Self::default();

    for (index, secret) in secrets.iter().enumerate() {
      handing.hand(index, secret, given)?;
    }

    handing
      .masks
      .sort_by(|(one, _), (other, _)| other.len().cmp(&one.len()).then(one.cmp(other)));
    handing.masks.dedup_by(|(one, _), (other, _)| one == other);
    Ok(handing)
  }

  fn hand(
    &mut self,
    index: usize,
    secret: &Secret,
    given: &BTreeMap<String, String>,
  ) -> Result<(), String> {
    let name = &secret.name;
    let stem = stem(name);

    if secret.way.env() {
      match &secret.fields {
        Some(fields) => {
          for (key, value) in fields {
            self.set(format!("{}{key}", secret.prefix), value.clone(), name, given)?;
          }
        }
        None => {
          let value = String::from_utf8(secret.bytes.clone())
            .map_err(|_| format!("The secret {name} is not text, so it is handed as a file alone."))?;

          self.set(format!("BMO_SECRET_{stem}"), value, name, given)?;
        }
      }
    }

    if secret.way.file() {
      let carried = Carried {
        carrier: format!("BMO_SECRET_BYTES_{index}"),
        path: format!("BMO_SECRET_{stem}_FILE"),
        file: index.to_string(),
      };
      let encoded = STANDARD.encode(&secret.bytes);

      self.set(carried.carrier.clone(), encoded.clone(), name, given)?;
      self.claim(&carried.path, given)?;
      self.mask(encoded, name);
      self.files.push(carried);
    }

    for (_, value) in secret.fields.iter().flatten() {
      self.mask(value.clone(), name);
    }

    if let Ok(text) = std::str::from_utf8(&secret.bytes) {
      self.mask(text.trim_end_matches(['\r', '\n']).to_owned(), name);

      for line in text.lines() {
        self.mask(line.trim_end_matches('\r').to_owned(), name);
      }
    }

    Ok(())
  }

  /// Claims `variable` for a secret, or says it is named twice.
  fn claim(&self, variable: &str, given: &BTreeMap<String, String>) -> Result<(), String> {
    let taken = given.contains_key(variable)
      || self.env.contains_key(variable)
      || self.files.iter().any(|carried| carried.path == variable);

    if taken {
      return Err(format!(
        "The variable {variable} is named twice among the call's env and its secrets: give a prefix."
      ));
    }

    Ok(())
  }

  fn set(
    &mut self,
    variable: String,
    value: String,
    name: &str,
    given: &BTreeMap<String, String>,
  ) -> Result<(), String> {
    self.claim(&variable, given)?;
    self.mask(value.clone(), name);
    self.env.insert(variable, value);
    Ok(())
  }

  fn mask(&mut self, value: String, name: &str) {
    if value.chars().count() >= FEWEST_MASKED {
      self.masks.push((value, name.to_owned()));
    }
  }

  /// `text` with every value handed replaced by the name of its secret, the longest value first.
  pub fn masked(&self, text: &str) -> String {
    self.masks.iter().fold(text.to_owned(), |text, (value, name)| {
      text.replace(value.as_str(), &format!("[secret {name}]"))
    })
  }
}
