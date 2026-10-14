//! Thin wrappers over [`horon_plugin::testing`] with this crate's test target dir.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn built_plugins_for(test: &str) -> Option<&'static Path> {
    horon_plugin::testing::built_plugins(Path::new(env!("CARGO_TARGET_TMPDIR")), test)
}

pub fn scratch(name: &str) -> PathBuf {
    horon_plugin::testing::scratch(Path::new(env!("CARGO_TARGET_TMPDIR")), name)
}
