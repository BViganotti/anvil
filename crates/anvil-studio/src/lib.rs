//! Embedded local Studio for Anvil. The HTTP server binds to 127.0.0.1 only,
//! serves original assets from the binary, and authenticates API calls with a
//! per-launch token.

pub mod api;
pub mod assets;
pub mod http;
pub mod json;
