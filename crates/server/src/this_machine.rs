//! The names this machine answers to, read from the system: its host name and the addresses of its own
//! interfaces. What decides that a node is this machine (docs/nodes.md, 「This machine」).

use std::net::IpAddr;

use sysinfo::{Networks, System};

#[derive(Debug, Clone, Default)]
pub struct ThisMachine {
  pub host_name: Option<String>,
  pub addresses: Vec<IpAddr>,
}

impl ThisMachine {
  pub fn new(host_name: Option<String>, addresses: Vec<IpAddr>) -> Self {
    Self {
      host_name,
      addresses,
    }
  }

  /// This machine as the system names it now.
  pub fn read() -> Self {
    let networks = Networks::new_with_refreshed_list();
    let addresses = networks
      .list()
      .values()
      .flat_map(|network| network.ip_networks().iter().map(|ip| ip.addr))
      .collect();

    Self {
      host_name: System::host_name(),
      addresses,
    }
  }

  /// Whether a host a node is reached at names this machine: a loopback address, localhost, this machine's
  /// host name, or one of its own addresses.
  pub fn names(&self, host: &str) -> bool {
    let host = host.trim();

    match host.parse::<IpAddr>() {
      Ok(address) => {
        let address = address.to_canonical();

        address.is_loopback() || self.addresses.iter().any(|own| own.to_canonical() == address)
      }
      Err(_) => {
        host.eq_ignore_ascii_case("localhost")
          || self.host_name.as_deref().is_some_and(|name| host.eq_ignore_ascii_case(name))
      }
    }
  }
}
