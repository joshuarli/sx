#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod decimal;
pub mod git;
pub mod github;
pub mod json;
pub mod guide;
pub mod kernel;
pub mod re;
pub mod semver;
mod tables;
pub mod text;
pub mod yaml;
