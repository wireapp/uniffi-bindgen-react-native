/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    process::Command,
};

use anyhow::{ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::Args;
use ubrn_bindgen::{AbiFlavor, BindingsArgs, OutputArgs, SourceArgs, SwitchArgs};
use ubrn_common::{mk_dir, run_cmd_quietly, write_file, CrateMetadata};

use super::NapiConfig;
use crate::{config::ProjectConfig, workspace};

#[derive(Args, Debug)]
pub(crate) struct BuildArgs {
    /// The crate to generate and build Node.js bindings for.
    #[clap(long = "crate")]
    crate_dir: Utf8PathBuf,

    /// Directory for the generated Typescript to put in.
    #[clap(long)]
    ts_dir: Utf8PathBuf,

    /// Directory for the generated low-level Rust bindings and generated NAPI crate.
    #[clap(long = "abi-dir", alias = "cpp-dir")]
    abi_dir: Utf8PathBuf,

    /// Optional uniffi.toml location.
    #[clap(long)]
    toml: Option<Utf8PathBuf>,

    /// Optional ubrn.config.yaml location.
    ///
    /// If omitted, this command will auto-discover `ubrn.config.yaml` when present.
    #[clap(long)]
    config: Option<Utf8PathBuf>,

    /// By default, bindgen will attempt to format generated code.
    #[clap(long)]
    no_format: bool,

    /// Build a release build.
    #[clap(long, short, default_value = "false")]
    release: bool,

    /// Use a specific build profile.
    ///
    /// This overrides the -r / --release flag if both are specified.
    #[clap(long, short)]
    profile: Option<String>,

    /// If the Rust library has already been built, then don't re-run cargo build.
    #[clap(long)]
    no_cargo: bool,
}

impl BuildArgs {
    pub(crate) fn run(&self) -> Result<()> {
        let napi_config = self.napi_config()?;
        let profile = CrateMetadata::profile(self.profile.as_deref(), self.release);
        let crate_ = CrateMetadata::try_from(self.crate_dir.clone())
            .with_context(|| format!("Failed to read crate metadata from {}", self.crate_dir))?;

        if !self.no_cargo {
            self.build_target_crate(&crate_, profile, &napi_config)?;
        }

        let library = crate_.library_path(None, profile, None);
        crate_.library_path_exists(&library).with_context(|| {
            format!(
                "Expected compiled Rust library at {} before generating NAPI bindings",
                library
            )
        })?;

        let generated_crate = self.abi_dir.clone();
        mk_dir(&generated_crate).with_context(|| {
            format!(
                "Failed to create generated NAPI crate directory {}",
                generated_crate
            )
        })?;
        let src_dir = generated_crate.join("src");
        mk_dir(&src_dir)
            .with_context(|| format!("Failed to create generated source dir {}", src_dir))?;

        let output = OutputArgs::new(&self.ts_dir, &src_dir, self.no_format);
        let source = SourceArgs::library(&library).with_config(self.uniffi_toml());
        let switches = SwitchArgs {
            flavor: AbiFlavor::Napi,
        };
        let bindings = BindingsArgs::new(switches.clone(), source, output);
        let mut restore_force_contract = false;
        if std::env::var("UNIFFI_FORCE_CONTRACT_VERSION").is_err() {
            if matches!(
                self.detect_scaffolding_contract_version(&crate_),
                Ok(version) if version < 30
            ) {
                std::env::set_var("UNIFFI_FORCE_CONTRACT_VERSION", "29");
                restore_force_contract = true;
            }
        }
        let modules_result = bindings.run(Some(&crate_.manifest_path().to_path_buf()));
        if restore_force_contract {
            std::env::remove_var("UNIFFI_FORCE_CONTRACT_VERSION");
        }
        let modules = modules_result.context("Failed to generate NAPI bindings")?;

        let entrypoint_path = generated_crate.join(switches.flavor.entrypoint());
        let entrypoint = ubrn_bindgen::generate_entrypoint(&switches, &crate_, &modules)
            .context("Failed to render NAPI entrypoint source")?;
        write_file(entrypoint_path.clone(), entrypoint)
            .with_context(|| format!("Failed to write NAPI entrypoint {}", entrypoint_path))?;

        let cargo_toml = self
            .render_cargo_toml(&generated_crate, &crate_, &napi_config)
            .context("Failed to write generated NAPI Cargo.toml")?;
        self.compile_generated_crate(&cargo_toml, profile)
            .context("Failed to compile generated NAPI crate")?;
        self.stage_node_addon(&cargo_toml, &self.ts_dir, profile)
            .context("Failed to stage compiled NAPI addon for Node.js")?;
        Ok(())
    }

    fn detect_scaffolding_contract_version(&self, crate_: &CrateMetadata) -> Result<u32> {
        let metadata = CrateMetadata::cargo_metadata(crate_.manifest_path().to_path_buf())?;
        let root_pkg = metadata
            .packages
            .iter()
            .find(|pkg| pkg.manifest_path == crate_.manifest_path())
            .with_context(|| {
                format!(
                    "Failed to find root package for {} in cargo metadata",
                    crate_.manifest_path()
                )
            })?;

        let resolve = metadata
            .resolve
            .as_ref()
            .context("Cargo metadata did not include dependency resolution graph")?;
        let packages_by_id: HashMap<String, _> = metadata
            .packages
            .iter()
            .map(|pkg| (pkg.id.to_string(), pkg))
            .collect();
        let nodes_by_id: HashMap<String, _> = resolve
            .nodes
            .iter()
            .map(|node| (node.id.to_string(), node))
            .collect();

        let mut queue = VecDeque::from([root_pkg.id.to_string()]);
        let mut visited = HashSet::new();

        while let Some(node_id) = queue.pop_front() {
            if !visited.insert(node_id.clone()) {
                continue;
            }

            if let Some(pkg) = packages_by_id.get(&node_id) {
                if (pkg.name == "uniffi" || pkg.name == "uniffi_core")
                    && pkg.version.major == 0
                    && pkg.version.minor < 30
                {
                    return Ok(29);
                }
            }

            if let Some(node) = nodes_by_id.get(&node_id) {
                queue.extend(node.deps.iter().map(|dep| dep.pkg.to_string()));
            }
        }

        Ok(30)
    }

    fn uniffi_toml(&self) -> Option<Utf8PathBuf> {
        self.toml.clone().filter(|toml| toml.exists())
    }

    fn napi_config(&self) -> Result<NapiConfig> {
        let config_file = if let Some(config) = &self.config {
            Some(config.clone())
        } else {
            workspace::ubrn_config_yaml().ok()
        };

        if let Some(config_file) = config_file {
            let config = ProjectConfig::try_from(config_file.clone())
                .with_context(|| format!("Failed to load configuration from {}", config_file))?;
            Ok(config.napi)
        } else {
            Ok(Default::default())
        }
    }

    fn build_target_crate(
        &self,
        crate_: &CrateMetadata,
        profile: &str,
        config: &NapiConfig,
    ) -> Result<()> {
        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .args(["--manifest-path", crate_.manifest_path().as_str()])
            .current_dir(crate_.crate_dir());
        Self::apply_feature_config(&mut cmd, config);
        if profile != "debug" {
            cmd.args(["--profile", profile]);
        }
        run_cmd_quietly(&mut cmd).with_context(|| {
            format!(
                "Failed running cargo build for crate {}",
                crate_.manifest_path()
            )
        })?;
        Ok(())
    }

    fn render_cargo_toml(
        &self,
        generated_crate: &Utf8Path,
        crate_under_test: &CrateMetadata,
        config: &NapiConfig,
    ) -> Result<Utf8PathBuf> {
        let src = include_str!("Cargo.napi.template.toml");
        let cargo_toml = generated_crate.join("Cargo.toml");

        let crate_path = pathdiff::diff_utf8_paths(crate_under_test.crate_dir(), generated_crate)
            .expect("Should be able to calculate a relative path");
        let crate_dependency =
            Self::crate_dependency_line(crate_under_test.package_name(), &crate_path, config);
        let cargo_toml_src = src
            .replace(
                "{{crate_lib_name}}",
                &crate_under_test.package_name().replace('-', "_"),
            )
            .replace("{{crate_dependency}}", &crate_dependency);

        write_file(cargo_toml.clone(), cargo_toml_src)
            .with_context(|| format!("Failed to write generated Cargo.toml {}", cargo_toml))?;

        let build_rs = generated_crate.join("build.rs");
        write_file(build_rs.clone(), "fn main() { napi_build::setup(); }\n")
            .with_context(|| format!("Failed to write generated build script {}", build_rs))?;
        Ok(cargo_toml)
    }

    fn compile_generated_crate(&self, cargo_toml: &Utf8Path, profile: &str) -> Result<()> {
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
        mk_dir(&out_dir)
            .with_context(|| format!("Failed to create NAPI output directory {}", out_dir))?;

        let addon_path = out_dir.join("index.node");
        fs::copy(native_library.as_std_path(), addon_path.as_std_path()).with_context(|| {
            format!(
                "Failed to copy native addon from {} to {}",
                native_library, addon_path
            )
        })?;
        Self::codesign_addon_if_needed(&addon_path)
            .with_context(|| format!("Failed to codesign staged addon at {}", addon_path))?;

        let loader = r#"// @ts-nocheck
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const native = require("./index.node");
export default native;
"#;
        let loader_path = out_dir.join("index.js");
        write_file(loader_path.clone(), loader)
            .with_context(|| format!("Failed to write NAPI loader {}", loader_path))?;
        Ok(())
    }

    fn codesign_addon_if_needed(addon_path: &Utf8Path) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("codesign");
            cmd.args(["-s", "-", "-f", addon_path.as_str()]);
            run_cmd_quietly(&mut cmd).with_context(|| {
                format!(
                    "Failed running codesign for generated NAPI addon {}",
                    addon_path
                )
            })?;
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = addon_path;
        }
        Ok(())
    }

    fn apply_feature_config(cmd: &mut Command, config: &NapiConfig) {
        if let Some(features) = config.features.as_ref() {
            cmd.arg("--features").arg(features.join(","));
        }
        if let Some(default_features) = config.default_features {
            if default_features {
                cmd.arg("--default-features");
            } else {
                cmd.arg("--no-default-features");
            }
        }
    }

    fn crate_dependency_line(
        package_name: &str,
        crate_path: &Utf8Path,
        config: &NapiConfig,
    ) -> String {
        let mut entries = vec![format!("path = \"{}\"", crate_path)];
        if let Some(features) = config.features.as_ref() {
            let features = features
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", ");
            entries.push(format!("features = [{features}]"));
        }
        if let Some(default_features) = config.default_features {
            entries.push(format!("default-features = {default_features}"));
        }
        format!("{package_name} = {{ {} }}", entries.join(", "))
    }
}
