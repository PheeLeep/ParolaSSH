//! One-click tasks: a saved command, run on one host, shown before it runs.
//!
//! * `catalog`  — the tasks the app ships, constructed per OS in Rust.
//! * `danger`   — reading a command for the shapes that end badly.
//! * `model`    — what a task is, and what a press would actually execute.
//! * `store`    — `tasks.json`, owner-only, beside the address book.
//! * `commands` — the Tauri surface.
//!
//! Two things separate this from a terminal with a bookmark list. The command
//! is **always shown before it runs**, wrapper and all, so what is approved is
//! what executes. And elevation is a **per-task choice** the operator makes
//! once and can override per press — a task that says it runs as root either
//! does, or errors, never quietly runs as someone else.

pub mod catalog;
pub mod commands;
pub mod danger;
pub mod model;
pub mod store;

pub use model::{TaskDraft, TaskRecord, TaskScope};
pub use store::TaskStore;
