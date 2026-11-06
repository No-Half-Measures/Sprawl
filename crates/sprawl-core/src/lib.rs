pub mod admin;
pub mod backend;
pub mod cache;
pub mod changes;
pub mod cli;
pub mod dates;
pub mod export;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
pub mod mft;
pub mod recycle;
pub mod scanner;
pub mod search;
pub mod shell;
pub use backend::{Backend, BackendEvent};

pub const CATEGORIES_JSON: &str = include_str!("../categories.json");
