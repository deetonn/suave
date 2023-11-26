//! The library for all things local communication. We support a variety of things for
//! multi-process communication and interprocess **communication**.
//!
//! This includes **lock files**, **named pipes** like in the `pipe` crate.
//!
//! To contribute head over to github, all help is welcome!

/// Anything pipe related, including lock files, shared pipes etc...
pub mod pipe;

/// Anything related to clipboard IPC.
pub mod clipboard;
