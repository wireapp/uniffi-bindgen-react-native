/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use anyhow::Result;

use ubrn_cli::test_utils::{cargo_build, fixtures_dir, run_cli};
use ubrn_cli_testing::{
    assert_commands, assert_files, shim_file_str, shim_path, with_fixture, Command, File,
};
use ubrn_common::get_recorded_commands;

#[test]
fn test_happy_path_ios() -> Result<()> {
    let target_crate = cargo_build("arithmetic")?;
    let fixtures_dir = fixtures_dir();
    with_fixture(fixtures_dir.clone(), "defaults", |_fixture_dir| {
        // Set up file shims
        shim_path("package.json", fixtures_dir.join("defaults/package.json"));
        shim_path(
            "ubrn.config.yaml",
            fixtures_dir.join("defaults/ubrn.config.yaml"),
        );
        shim_path("rust/shim/Cargo.toml", target_crate.manifest_path());
        shim_path("rust/shim", target_crate.project_root());
        shim_path(
            "libarithmetical.a",
            target_crate.library_path(None, "debug", None),
        );

        // Run the command under test
        run_cli("ubrn build ios --and-generate --config ubrn.config.yaml --targets aarch64-apple-ios,x86_64-apple-ios")?;

        // Assert the expected commands were executed
        assert_commands(&[
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "arithmetic/Cargo.toml")
                .arg_pair("--target", "aarch64-apple-ios"),
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "arithmetic/Cargo.toml")
                .arg_pair("--target", "x86_64-apple-ios"),
            Command::new("xcodebuild")
                .arg("-create-xcframework")
                .arg_pair_suffix("-library", "aarch64-apple-ios/debug/libarithmetical.a")
                .arg_pair_suffix("-library", "x86_64-apple-ios/debug/libarithmetical.a")
                .arg_pair_suffix("-output", "DefaultFixtureFramework.xcframework"),
        ]);

        assert_files(&[
            // This is the entry point to the module. It calls into the generated installRustCrate()
            // method.
            File::new("src/index.tsx")
                .contains("installer.installRustCrate()")
                .contains("import * as arithmetic from './generated/arithmetic'")
                .contains("export * from './generated/arithmetic'")
                .contains("arithmetic.default.initialize()"),
            // This is the file that specifies installRustCrate method.
            // It is the entry point to the RN codegen.
            File::new("src/NativeDefaultFixture.ts").contains(
                "export default TurboModuleRegistry.getEnforcing<Spec>('DefaultFixture')",
            ),
            // These next two files hook into the RN codegen generated files
            File::new("ios/DefaultFixture.h")
                .contains("#import \"DefaultFixtureSpec.h\"")
                .contains("@interface DefaultFixture : NSObject <NativeDefaultFixtureSpec>"),
            File::new("ios/DefaultFixture.mm")
                .contains("#import \"DefaultFixture.h\"")
                .contains("__hostFunction_DefaultFixture_installRustCrate")
                .contains("NativeDefaultFixtureSpecJS")
                .contains("defaultfixture::installRustCrate"),
            // These next two are the cross platform entrypoint to the bindings.
            File::new("cpp/default-fixture.h").contains("namespace defaultfixture"),
            File::new("cpp/default-fixture.cpp")
                .contains("namespace defaultfixture")
                .contains("#include \"default-fixture.h\"")
                .contains("#include \"generated/arithmetic.hpp\"")
                .contains("NativeArithmetic::registerModule"),
            // Finally, the bindings. We have tested the content of these in other tests.
            File::new("src/generated/arithmetic.ts"),
            File::new("src/generated/arithmetic-ffi.ts"),
            File::new("cpp/generated/arithmetic.cpp"),
            File::new("cpp/generated/arithmetic.hpp"),
            // Assorted build files
            // This is the podspec that tells iOS about the Rust framework generated with xcodebuild.
            File::new("DefaultFixture.podspec")
                .contains("s.vendored_frameworks = \"DefaultFixtureFramework.xcframework\""),
        ]);

        Ok(())
    })
}

#[test]
fn test_happy_path_android() -> Result<()> {
    let target_crate = cargo_build("arithmetic")?;
    let fixtures_dir = fixtures_dir();
    with_fixture(fixtures_dir.clone(), "defaults", |_fixture_dir| {
        // Set up file shims
        shim_path("package.json", fixtures_dir.join("defaults/package.json"));
        shim_path(
            "ubrn.config.yaml",
            fixtures_dir.join("defaults/ubrn.config.yaml"),
        );
        shim_path("rust/shim/Cargo.toml", target_crate.manifest_path());
        shim_path("rust/shim", target_crate.project_root());
        shim_path(
            "libarithmetical.a",
            target_crate.library_path(None, "debug", None),
        );

        // Run the command under test
        run_cli("ubrn build android --and-generate --config ubrn.config.yaml")?;

        // Assert the expected commands were executed
        assert_commands(&[
            Command::new("cargo")
                .arg("ndk")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml")
                .arg_pair_suffix("--target", "arm64-v8a")
                .arg_pair("--platform", "23")
                .arg("--")
                .arg("build"),
            Command::new("cargo")
                .arg("ndk")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml")
                .arg_pair_suffix("--target", "armeabi-v7a")
                .arg_pair("--platform", "23")
                .arg("--")
                .arg("build"),
            Command::new("cargo")
                .arg("ndk")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml")
                .arg_pair_suffix("--target", "x86")
                .arg_pair("--platform", "23")
                .arg("--")
                .arg("build"),
            Command::new("cargo")
                .arg("ndk")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml")
                .arg_pair_suffix("--target", "x86_64")
                .arg_pair("--platform", "23")
                .arg("--")
                .arg("build"),
            Command::new("prettier"),
            Command::new("clang-format"),
        ]);

        assert_files(&[
            // This is the entry point to the module. It calls into the generated installRustCrate()
            // method.
            File::new("src/index.tsx")
                .contains("installer.installRustCrate()")
                .contains("import * as arithmetic from './generated/arithmetic'")
                .contains("export * from './generated/arithmetic'")
                .contains("arithmetic.default.initialize()"),
            // This is the file that specifies installRustCrate method.
            // It is the entry point to the RN codegen.
            File::new("src/NativeDefaultFixture.ts").contains(
                "export default TurboModuleRegistry.getEnforcing<Spec>('DefaultFixture')",
            ),
            // // These next two files hook into the RN codegen generated files
            File::new("android/src/main/java/com/defaultfixture/DefaultFixturePackage.kt")
                .contains("class DefaultFixturePackage")
                .contains("DefaultFixtureModule.NAME"),
            File::new("android/src/main/java/com/defaultfixture/DefaultFixtureModule.kt")
                .contains("class DefaultFixtureModule")
                .contains("System.loadLibrary(\"default-fixture\")"),
            File::new("android/build.gradle")
                .contains("jsRootDir = file(\"../src/\")")
                .contains("libraryName = \"DefaultFixture\"")
                .contains("codegenJavaPackageName = \"com.defaultfixture\""),
            File::new("android/CMakeLists.txt")
                .contains("../cpp/default-fixture.cpp")
                .contains("../cpp/generated/arithmetic.cpp")
                .contains("cpp-adapter.cpp"),
            File::new("android/cpp-adapter.cpp")
                .contains("Java_com_defaultfixture_DefaultFixtureModule_nativeInstallRustCrate")
                .contains("defaultfixture::installRustCrate"),
            // These next two are the cross platform entrypoint to the bindings.
            File::new("cpp/default-fixture.h").contains("namespace defaultfixture"),
            File::new("cpp/default-fixture.cpp")
                .contains("namespace defaultfixture")
                .contains("#include \"default-fixture.h\"")
                .contains("#include \"generated/arithmetic.hpp\"")
                .contains("NativeArithmetic::registerModule"),
            // Finally, the bindings. We have tested the content of these in other tests.
            File::new("src/generated/arithmetic.ts"),
            File::new("src/generated/arithmetic-ffi.ts"),
            File::new("cpp/generated/arithmetic.cpp"),
            File::new("cpp/generated/arithmetic.hpp"),
            // Assorted glue and build files.
            // This is the AndroidManifest which tells the android app about this package.
            File::new("android/src/main/AndroidManifest.xml")
                .contains("package=\"com.defaultfixture\""),
        ]);

        Ok(())
    })
}

#[test]
fn test_happy_path_web() -> Result<()> {
    let target_crate = cargo_build("arithmetic")?;
    let fixtures_dir = fixtures_dir();
    with_fixture(fixtures_dir.clone(), "defaults", |_fixture_dir| {
        // Set up file shims
        shim_path("package.json", fixtures_dir.join("defaults/package.json"));
        shim_path(
            "ubrn.config.yaml",
            fixtures_dir.join("defaults/ubrn.config.yaml"),
        );
        shim_path("rust/shim/Cargo.toml", target_crate.manifest_path());
        shim_path("rust/shim", target_crate.project_root());

        shim_path("rust_modules/wasm/Cargo.toml", target_crate.manifest_path());
        shim_path(
            "libarithmetical.a",
            target_crate.library_path(None, "debug", None),
        );

        // Run the command under test
        run_cli("ubrn build web --config ubrn.config.yaml")?;

        // Assert the expected commands were executed
        assert_commands(&[
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml"),
            Command::new("prettier"),
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "rust_modules/wasm/Cargo.toml")
                .arg_pair("--target", "wasm32-unknown-unknown"),
            Command::new("wasm-bindgen")
                .arg_pair("--target", "web")
                .arg("--omit-default-module-path")
                .arg_pair("--out-name", "index")
                .arg_pair_suffix("--out-dir", "src/generated/wasm-bindgen")
                .arg_suffix("wasm32-unknown-unknown/debug/arithmetical.wasm"),
        ]);

        assert_files(&[
            // This is the entry point to the module. It calls into the generated installRustCrate()
            // method.
            File::new("src/index.web.ts")
                .contains("import * as arithmetic from './generated/arithmetic'")
                .contains("export * from './generated/arithmetic'")
                .contains("arithmetic.default.initialize()"),
            // Finally, the bindings. We have tested the content of these in other tests.
            File::new("src/generated/arithmetic.ts"),
            File::new("rust_modules/wasm/src/lib.rs"),
            File::new("rust_modules/wasm/src/arithmetic_module.rs"),
            File::new("rust_modules/wasm/Cargo.toml")
                .contains("[workspace]")
                .contains("uniffi-example-arithmetic = { path = \"../../rust/shim\" }"),
        ]);

        Ok(())
    })
}

#[test]
fn test_happy_path_napi_multiple_targets() -> Result<()> {
    let target_crate = cargo_build("arithmetic")?;
    let fixtures_dir = fixtures_dir();
    with_fixture(fixtures_dir.clone(), "defaults", |_fixture_dir| {
        shim_path("package.json", fixtures_dir.join("defaults/package.json"));
        shim_file_str(
            "ubrn.config.yaml",
            r#"rust:
  directory: rust/shim
  manifestPath: Cargo.toml
napi:
  targets:
    - x86_64-unknown-linux-gnu
"#,
        );
        shim_path("rust/shim/Cargo.toml", target_crate.manifest_path());
        shim_path("rust/shim", target_crate.project_root());

        let host_library = target_crate.library_path(None, "debug", None);
        shim_path("libuniffi_example_arithmetic_napi.so", &host_library);
        shim_path("libuniffi_example_arithmetic_napi.dylib", &host_library);

        run_cli(
            "ubrn generate napi build --config ubrn.config.yaml --targets x86_64-unknown-linux-gnu,aarch64-apple-darwin --crate rust/shim/Cargo.toml --ts-dir src/generated --abi-dir native/generated",
        )?;

        assert_commands(&[
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "fixtures/arithmetic/Cargo.toml"),
            Command::new("prettier"),
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "native/generated/Cargo.toml")
                .arg_pair("--target", "x86_64-unknown-linux-gnu"),
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "native/generated/Cargo.toml")
                .arg_pair("--target", "aarch64-apple-darwin"),
        ]);

        assert_files(&[
            File::new("src/generated/arithmetic.ts")
                .contains("let uniffiInitialized = false;")
                .contains("uniffiEnsureInitialized();"),
            File::new("native/generated/Cargo.toml")
                .contains("name = \"uniffi-napi-bindings\"")
                .contains("name = \"uniffi_example_arithmetic_napi\"")
                .contains("napi-build = \"2\""),
            File::new("native/generated/src/lib.rs"),
            File::new("src/generated/napi-bindings/index.js")
                .contains("\"linux-x64\": \"./linux-x64.node\"")
                .contains("\"darwin-arm64\": \"./darwin-arm64.node\"")
                .contains("const target = `${process.platform}-${process.arch}`")
                .contains("Unsupported N-API target"),
        ]);

        Ok(())
    })
}

#[test]
fn test_happy_path_napi_skip_build() -> Result<()> {
    let target_crate = cargo_build("arithmetic")?;
    let fixtures_dir = fixtures_dir();
    with_fixture(fixtures_dir.clone(), "defaults", |_fixture_dir| {
        shim_path("package.json", fixtures_dir.join("defaults/package.json"));
        shim_file_str(
            "ubrn.config.yaml",
            r#"rust:
  directory: rust/shim
  manifestPath: Cargo.toml
"#,
        );
        shim_path("rust/shim/Cargo.toml", target_crate.manifest_path());
        shim_path("rust/shim", target_crate.project_root());

        let host_library = target_crate.library_path(None, "debug", None);
        shim_path("libuniffi_example_arithmetic_napi.so", &host_library);

        run_cli(
            "ubrn generate napi build --config ubrn.config.yaml --skip-build --targets x86_64-unknown-linux-gnu --crate rust/shim/Cargo.toml --ts-dir src/generated --abi-dir native/generated",
        )?;

        let commands = get_recorded_commands();
        assert_eq!(
            commands.len(),
            2,
            "expected only prettier and generated crate build"
        );
        assert_commands(&[
            Command::new("prettier"),
            Command::new("cargo")
                .arg("build")
                .arg_pair_suffix("--manifest-path", "native/generated/Cargo.toml")
                .arg_pair("--target", "x86_64-unknown-linux-gnu"),
        ]);

        assert_files(&[
            File::new("src/generated/arithmetic.ts")
                .contains("let uniffiInitialized = false;")
                .contains("uniffiEnsureInitialized();"),
            File::new("src/generated/napi-bindings/index.js")
                .contains("\"linux-x64\": \"./linux-x64.node\"")
                .contains("Unsupported N-API target"),
        ]);

        Ok(())
    })
}
