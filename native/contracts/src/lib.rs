//! Desktop Pet's host-neutral boundary: normalized models, atomic domain
//! access, registered integration/actions, opaque preference patches, and a
//! shared profile writer for lifecycle and preference transactions.
//! Host wire parsing, endpoints, registries, UI frameworks and updater protocols
//! deliberately live outside this crate.

mod extensions;
mod models;
mod observation;
mod ports;
mod preferences_owner;
pub mod profile_store;

pub use extensions::*;
pub use models::*;
pub use observation::*;
pub use ports::*;
pub use profile_store::{LifecycleLock, ProfileStore};

pub const INTEGRATION_API_VERSION: u32 = 1;
