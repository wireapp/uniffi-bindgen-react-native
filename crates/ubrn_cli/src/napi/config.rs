/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use std::{
    env::consts::{ARCH, OS},
    fmt::Display,
    str::FromStr,
};

use anyhow::{Error, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NapiConfig {
    #[serde(default)]
    pub(crate) features: Option<Vec<String>>,

    #[serde(default)]
    pub(crate) default_features: Option<bool>,

    #[serde(default)]
    pub(crate) targets: Option<Vec<Target>>,
}

impl Default for NapiConfig {
    fn default() -> Self {
        Self {
            features: None,
            default_features: None,
            targets: None,
        }
    }
}

impl NapiConfig {
    pub(crate) fn targets(&self) -> Result<Vec<Target>> {
        match &self.targets {
            Some(targets) if targets.is_empty() => {
                anyhow::bail!("The `napi.targets` list must not be empty")
            }
            Some(targets) => Ok(targets.clone()),
            None => Ok(vec![Target::host()?]),
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) enum Target {
    Aarch64AppleDarwin,
    X86_64UnknownLinuxGnu,
}

impl Target {
    pub(crate) fn triple(&self) -> &'static str {
        match self {
            Self::Aarch64AppleDarwin => "aarch64-apple-darwin",
            Self::X86_64UnknownLinuxGnu => "x86_64-unknown-linux-gnu",
        }
    }

    pub(crate) fn loader_key(&self) -> &'static str {
        match self {
            Self::Aarch64AppleDarwin => "darwin-arm64",
            Self::X86_64UnknownLinuxGnu => "linux-x64",
        }
    }

    pub(crate) fn addon_filename(&self) -> String {
        format!("{}.node", self.loader_key())
    }

    pub(crate) fn is_darwin(&self) -> bool {
        matches!(self, Self::Aarch64AppleDarwin)
    }

    fn host() -> Result<Self> {
        match (OS, ARCH) {
            ("macos", "aarch64") => Ok(Self::Aarch64AppleDarwin),
            ("linux", "x86_64") => Ok(Self::X86_64UnknownLinuxGnu),
            _ => anyhow::bail!(
                "Unsupported host target {OS}/{ARCH}. Configure `napi.targets` with one of: aarch64-apple-darwin, x86_64-unknown-linux-gnu"
            ),
        }
    }
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "aarch64-apple-darwin" | "darwin-aarch64" | "darwin-arm64" => Self::Aarch64AppleDarwin,
            "x86_64-unknown-linux-gnu" | "linux-x86_64" | "linux-x64" => {
                Self::X86_64UnknownLinuxGnu
            }
            _ => return Err(anyhow::anyhow!("Unsupported target: '{s}'")),
        })
    }
}

impl TryFrom<String> for Target {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.triple())
    }
}
