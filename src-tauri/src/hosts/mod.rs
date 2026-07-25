//! Saved connections: the address book, not the connection itself.
//!
//! A record here says *where* a host is and *how* to authenticate to it, never
//! *with what*. Secrets live in `remote::secrets` for the lifetime of the app;
//! this module's JSON file is safe to sync, back up, or read aloud.

pub mod commands;
pub mod model;
pub mod store;

pub use model::{AuthMethod, HostDraft, HostRecord};
pub use store::HostStore;
