/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */

use anyhow::Result;
use clap::{Args, Subcommand};
use ubrn_bindgen::{OutputArgs, SourceArgs, SwitchArgs};

use super::build::BuildArgs;

#[derive(Args, Debug)]
pub(crate) struct CmdArg {
    #[clap(subcommand)]
    cmd: Cmd,
}
impl CmdArg {
    pub(crate) fn run(&self) -> Result<()> {
        self.cmd.run()
    }
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Generate Typescript and napi-rs Rust bindings.
    Bindings(BindingsArgs),

    /// Generate bindings, build the generated NAPI crate, and stage a runtime loader plus target-specific addons.
    Build(BuildArgs),
}

impl Cmd {
    fn run(&self) -> Result<()> {
        match self {
            Self::Bindings(b) => {
                let b = ubrn_bindgen::BindingsArgs::from(b);
                b.run(None)?;
                Ok(())
            }
            Self::Build(b) => b.run(),
        }
    }
}

#[derive(Args, Debug)]
struct BindingsArgs {
    #[command(flatten)]
    pub(crate) source: SourceArgs,
    #[command(flatten)]
    pub(crate) output: OutputArgs,
}

impl From<&BindingsArgs> for ubrn_bindgen::BindingsArgs {
    fn from(value: &BindingsArgs) -> Self {
        ubrn_bindgen::BindingsArgs::new(
            SwitchArgs {
                flavor: ubrn_bindgen::AbiFlavor::Napi,
            },
            value.source.clone(),
            value.output.clone(),
        )
    }
}
