//! Financial Structural PRAMAgraph Signal.
//!
//! Domain-owned market contracts and adapters around the domain-blind PRAMA kernel.

pub mod calibration;
pub mod canonical;
pub mod contracts;
pub mod corpus;
pub mod cross_asset;
pub mod dynamics;
pub mod engine;
pub mod historical;
pub mod logging;
pub mod native_response;
pub mod observation;
pub mod provider;
pub mod resolver;
pub mod server;
pub mod service;
pub mod signal;
pub mod structural;
pub mod technical;

pub use contracts::*;
pub use logging::*;
pub use native_response::*;
pub use signal::*;
pub use technical::*;

// Re-export version constants for cross-asset module
pub use engine::ENGINE_VERSION;
pub use structural::STRUCTURAL_VECTOR_VERSION;
