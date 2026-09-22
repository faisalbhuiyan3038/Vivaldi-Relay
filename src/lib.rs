// lib.rs — shared library crate for vivaldi_mod_interceptor
// Both the `interceptor` binary and the `webui` binary import from here.

pub mod bundler;
pub mod config;
pub mod discovery;
pub mod launcher;
pub mod os_hook;
pub mod patcher;
pub mod webui;
