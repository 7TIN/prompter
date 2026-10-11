//! Prompter: a small desktop window that rewrites a rough prompt into a clean
//! one.
//!
//! [`app`] holds the window and everything it owns. This crate root exists so
//! the UI integration tests can drive the production view rather than a fixture.

pub mod app;
pub mod rewrite;
pub mod style;
pub mod theme;
