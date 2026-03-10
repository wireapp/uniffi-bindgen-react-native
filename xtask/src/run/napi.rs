/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use std::{fs, process::Command};

use anyhow::{ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ubrn_common::{run_cmd_quietly, CrateMetadata};

use super::{generate_bindings::render_entrypoint, NodeJs, RunCmd};
use crate::bootstrap::{Bootstrap, YarnCmd};

pub(crate) struct Napi;

impl Napi {
    pub(crate) fn run(&self, cmd: &RunCmd) -> Result<()> {
        YarnCmd
            .ensure_ready()
            .context("Failed to prepare Node.js dependencies with yarn")?;
        let _so_file = self.prepare_library_path(cmd)?;
        let js_file = cmd
            .js_file
            .file
            .canonicalize_utf8()
            .context(format!("{} expected, but wasn't there", &cmd.js_file.file))?;
        NodeJs
            .tsx(&js_file, false)
            .with_context(|| format!("Failed to execute test entrypoint {}", js_file))?;
        Ok(())
    }

    fn prepare_library_path(&self, cmd: &RunCmd) -> Result<Option<Utf8PathBuf>> {
        let switches = &cmd.switches;
        let clean = cmd.clean;

        match (&cmd.crate_, &cmd.cpp_binding, &cmd.generate_bindings) {
            (Some(crate_), None, Some(bindings)) => {
                let profile = crate_.profile();
                let crate_ = crate_
                    .cargo_build(clean)
                    .context("Failed to build target Rust crate")?;
                let crate_lib = crate_.library_path(None, profile, None);

                let generated_crate = bindings.abi_dir();
                let src_dir = generated_crate.join("src");
                ubrn_common::mk_dir(&src_dir).with_context(|| {
                    format!("Failed to create generated src dir at {}", src_dir)
                })?;

                let modules = bindings
                    .render_into(
                        &crate_lib,
                        switches,
                        &crate_.manifest_path().to_path_buf(),
                        &bindings.ts_dir(),
                        &src_dir,
                    )
                    .context("Failed to render generated bindings")?;

                let lib_rs = generated_crate.join(switches.flavor.entrypoint());
                render_entrypoint(switches, &lib_rs, &crate_, &modules)
                    .with_context(|| format!("Failed to write NAPI Rust entrypoint {}", lib_rs))?;

                let canonical_generated_crate =
                    generated_crate.canonicalize_utf8().with_context(|| {
                        format!(
                            "Failed to canonicalize generated crate directory {}",
                            generated_crate
                        )
                    })?;
                let cargo_toml = self
                    .render_cargo_toml(&canonical_generated_crate, &crate_)
                    .context("Failed to render generated Cargo.toml for NAPI crate")?;
                self.compile_napi(&cargo_toml, profile)
                    .context("Failed to compile generated NAPI crate")?;
                self.stage_node_addon(&cargo_toml, &bindings.ts_dir(), profile)
                    .context("Failed to stage compiled NAPI addon for Node.js")?;
                Ok(None)
            }
            (_, _, _) => Ok(None),
        }
    }

    fn render_cargo_toml(
        &self,
        generated_crate: &Utf8Path,
        crate_under_test: &CrateMetadata,
    ) -> Result<Utf8PathBuf> {
        let src = include_str!("Cargo.napi.template.toml");
        let cargo_toml = generated_crate.join("Cargo.toml");

        let crate_name = crate_under_test.package_name().replace('-', "_");
        let cargo_toml_src = src
            .replace("{{crate_name}}", crate_under_test.package_name())
            .replace("{{crate_lib_name}}", &crate_name)
            .replace(
                "{{crate_path}}",
                pathdiff::diff_utf8_paths(crate_under_test.crate_dir(), generated_crate)
                    .expect("Should be able to find a relative path")
                    .as_str(),
            );

        ubrn_common::write_file(&cargo_toml, cargo_toml_src)
            .with_context(|| format!("Failed to write generated Cargo.toml to {}", cargo_toml))?;
        let build_rs = generated_crate.join("build.rs");
        ubrn_common::write_file(build_rs.clone(), "fn main() { napi_build::setup(); }\n")
            .with_context(|| format!("Failed to write build script to {}", build_rs))?;
        Ok(cargo_toml)
    }

    fn compile_napi(&self, cargo_toml: &Utf8Path, profile: &str) -> Result<()> {
        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .args(["--manifest-path", cargo_toml.as_str()]);
        if profile != "debug" {
            cmd.args(["--profile", profile]);
        }
        run_cmd_quietly(&mut cmd).with_context(|| {
            format!(
                "Failed running cargo build for generated NAPI crate {}",
                cargo_toml
            )
        })?;
        Ok(())
    }

    fn stage_node_addon(
        &self,
        cargo_toml: &Utf8Path,
        ts_dir: &Utf8Path,
        profile: &str,
    ) -> Result<()> {
        let metadata = CrateMetadata::try_from(cargo_toml.to_path_buf())
            .with_context(|| format!("Failed to load crate metadata from {}", cargo_toml))?;
        let native_library = metadata.library_path(None, profile, None);
        ensure!(
            native_library.exists(),
            "Expected compiled native library at {}, but it does not exist",
            native_library
        );

        let out_dir = ts_dir.join("napi-bindings");
        ubrn_common::mk_dir(&out_dir)
            .with_context(|| format!("Failed to create NAPI output directory {}", out_dir))?;

        let addon_path = out_dir.join("index.node");
        fs::copy(native_library.as_std_path(), addon_path.as_std_path()).with_context(|| {
            format!(
                "Failed to copy native addon from {} to {}",
                native_library, addon_path
            )
        })?;

        let loader = r#"// @ts-nocheck
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const native = require("./index.node");
export default native;
"#;
        let loader_path = out_dir.join("index.js");
        ubrn_common::write_file(loader_path.clone(), loader)
            .with_context(|| format!("Failed to write NAPI ESM loader {}", loader_path))?;
        Ok(())
    }
}
