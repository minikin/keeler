//! The pipeline gate: does the evidence account for every ticked task?
//!
//! One module per input — the decision at the core, and beside it the
//! parsers that feed it — so each can be built and tested without touching
//! the others' region.

pub mod backlog;
pub mod decision;
pub mod records;
