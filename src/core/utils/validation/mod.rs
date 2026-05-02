//! Trading validation — pure logic, no I/O. Lives in `core/` because
//! it operates on domain types (orders, prices) without external dependencies.

pub mod orders;
pub mod price;

pub use orders::*;
pub use price::*;
