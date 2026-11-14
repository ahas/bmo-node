//! A node's owners, read for a run (docs/nodes.md, 「A person's node」): the address the configuration names
//! asked for each run with the token read out of its file for that one request, and the accounts its
//! `owners` list answered. Nothing of the answer is kept.

use std::path::PathBuf;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

/// Where a node's owners are read, and the file the token presented there is kept in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnersRead {
  /// The address asked, `{nodeName}` and `{caller}` standing for the node's and the caller's names.
  pub url: String,
  pub token_file: PathBuf,
}

fn encoded(value: &str) -> String {
  utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

/// An account as the read writes it, a number or a string alike; none for anything else.
fn account(value: &Value) -> Option<String> {
  match value {
    Value::Number(number) => Some(number.to_string()),
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    _ => None,
  }
}

impl OwnersRead {
  /// The address asked for `node` on behalf of `caller`.
  pub fn address(&self, node: &str, caller: &str) -> String {
    self.url.replace("{nodeName}", &encoded(node)).replace("{caller}", &encoded(caller))
  }

  /// The accounts that own `node`, none where it is no one's, or why they could not be read.
  pub async fn owners(
    &self,
    http: &reqwest::Client,
    node: &str,
    caller: &str,
  ) -> Result<Vec<String>, String> {
    let token = std::fs::read_to_string(&self.token_file)
      .map_err(|_| format!("the owners' read's token file {} cannot be read", self.token_file.display()))?;
    let answer = http
      .get(self.address(node, caller))
      .bearer_auth(token.trim())
      .send()
      .await
      .map_err(|why| format!("the owners' read did not answer: {why}"))?;
    let status = answer.status().as_u16();

    if status != 200 {
      return Err(format!("the owners' read answered {status}"));
    }

    let body = answer.json::<Value>().await.unwrap_or(Value::Null);
    let listed = body
      .get("owners")
      .and_then(Value::as_array)
      .ok_or_else(|| "the owners' read answered no owners list".to_owned())?;

    listed
      .iter()
      .map(|one| {
        account(one).ok_or_else(|| format!("the owners' read answered an owner that is no account: {one}"))
      })
      .collect()
  }
}
