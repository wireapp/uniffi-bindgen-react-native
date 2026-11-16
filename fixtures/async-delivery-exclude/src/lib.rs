/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */

//! Top-level functions that `asyncDelivery` excludes: they keep a synchronous
//! surface and run on a player on the calling thread, while everything else
//! goes over the port.

#[cfg(target_arch = "wasm32")]
extern crate uniffi_runtime_wasm as _;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

uniffi::setup_scaffolding!();

#[derive(uniffi::Record)]
pub struct VersionInfo {
    pub major: u32,
    pub label: String,
    pub tags: Vec<String>,
}

#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum Suite {
    Small,
    Large,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SuiteError {
    #[error("unknown suite {value}")]
    Unknown { value: u16 },
}

/// Excluded.
#[uniffi::export]
pub fn version() -> String {
    "1.2.3".to_owned()
}

/// Excluded: a record of plain values.
#[uniffi::export]
pub fn version_info() -> VersionInfo {
    VersionInfo {
        major: 1,
        label: "one".to_owned(),
        tags: vec!["a".to_owned(), "b".to_owned()],
    }
}

/// Excluded: an enum, and an error which must throw synchronously.
#[uniffi::export]
pub fn suite_from_u16(value: u16) -> Result<Suite, SuiteError> {
    match value {
        1 => Ok(Suite::Small),
        2 => Ok(Suite::Large),
        value => Err(SuiteError::Unknown { value }),
    }
}

/// Excluded: bytes in and out, through the synchronous player's buffers.
#[uniffi::export]
pub fn echo_bytes(bytes: Vec<u8>) -> Vec<u8> {
    bytes
}

/// Delivered.
#[uniffi::export]
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

static GLOBAL: AtomicU32 = AtomicU32::new(0);

/// Delivered: writes this instance's global.
#[uniffi::export]
pub fn write_global(value: u32) {
    GLOBAL.store(value, Ordering::SeqCst);
}

/// Excluded: reads the global of the instance it runs on.
#[uniffi::export]
pub fn read_global() -> u32 {
    GLOBAL.load(Ordering::SeqCst)
}

/// Delivered: objects still work beside the excluded functions.
#[derive(uniffi::Object)]
pub struct Counter {
    value: Mutex<u32>,
}

#[uniffi::export]
impl Counter {
    #[uniffi::constructor]
    pub fn new(value: u32) -> Self {
        Self {
            value: Mutex::new(value),
        }
    }

    pub fn increment(&self) -> u32 {
        let mut value = self.value.lock().unwrap();
        *value += 1;
        *value
    }
}
