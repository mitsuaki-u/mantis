pub mod addresses;
pub mod network;

pub use addresses::{get_network_addresses, validate_addresses, NetworkAddresses};
pub use network::NetworkConfig;
