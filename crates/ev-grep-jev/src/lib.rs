//! Jev's typed decision protocol over `OpenRouter` or direct `TypeSafe` HTTP.

extern crate alloc;

mod client;
mod configuration;
mod error;
mod protocol;
mod provider;
mod request_size;
mod telemetry;
mod transport;

pub use client::{Jev, MAX_QUESTIONS};
pub use error::{FailureKind, JevError, RequestError};
pub use protocol::{build_context_prompt_id, search_prompt_id};
pub use provider::Provider;
pub use request_size::count_context_request_bytes;
