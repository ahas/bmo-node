//! Prints the service's OpenAPI document as the repository keeps it:
//! `cargo run -p bmo-node-client --example openapi > docs/openapi.json`.

fn main() {
  print!("{}", bmo_node_client::openapi_json());
}
