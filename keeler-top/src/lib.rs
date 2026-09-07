//! The live board: one row per task of a spec, refreshed while a wave runs.
//!
//! The binary is the terminal; this library is where the decisions live, so
//! unit tests, property tests and cargo-mutants can reach them without a
//! terminal and without an agent.

pub mod stream;
