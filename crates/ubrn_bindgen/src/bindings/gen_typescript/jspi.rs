// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::collections::HashSet;

use anyhow::{ensure, Result};
use heck::ToUpperCamelCase;
use uniffi_bindgen::pipeline::general;

use super::config::JspiSelection;

struct Target<'a> {
    name: &'a str,
    has_callbacks: bool,
    members: Vec<Target<'a>>,
    // Raw ABI symbol and optional shared UniFFI poll symbol.
    exports: Vec<(&'a str, Option<&'a str>)>,
}

/// Apply the public callable selection with wasm2 ABI eligibility checks.
pub(super) fn resolve_wasm2(
    selection: &JspiSelection,
    namespace: &general::Namespace,
) -> Result<HashSet<String>> {
    let eligible: HashSet<_> = namespace
        .ffi_definitions
        .iter()
        .filter_map(|def| {
            let general::FfiDefinition::RustFunction(f) = def else {
                return None;
            };
            let supported = |ty: &general::FfiType| {
                matches!(
                    ty,
                    general::FfiType::UInt8
                        | general::FfiType::Int8
                        | general::FfiType::UInt16
                        | general::FfiType::Int16
                        | general::FfiType::UInt32
                        | general::FfiType::Int32
                        | general::FfiType::UInt64
                        | general::FfiType::Int64
                        | general::FfiType::Float32
                        | general::FfiType::Float64
                        | general::FfiType::Handle(_)
                        | general::FfiType::RustBuffer(_)
                )
            };
            (f.arguments.iter().all(|arg| supported(&arg.ty.ty))
                && f.return_type
                    .ty
                    .as_ref()
                    .is_none_or(|ret| supported(&ret.ty)))
            .then_some(f.name.0.as_str())
        })
        .collect();
    let mut targets = namespace_targets(namespace);
    for target in &mut targets {
        for member in &mut target.members {
            member.has_callbacks |= member
                .exports
                .iter()
                .any(|(symbol, _)| !eligible.contains(symbol));
        }
        target.has_callbacks |= target
            .exports
            .iter()
            .any(|(symbol, _)| !eligible.contains(symbol));
    }
    select_wasm2(selection, &targets)
}

fn select_wasm2(selection: &JspiSelection, targets: &[Target<'_>]) -> Result<HashSet<String>> {
    select(selection, targets, true)
        .map_err(|e| anyhow::anyhow!("wasm2 JSPI selection contains an unsupported callable: {e}"))
}

impl<'a> Target<'a> {
    fn callable(name: &'a str, callable: &'a general::Callable, has_callbacks: bool) -> Self {
        Self {
            name,
            has_callbacks,
            members: vec![],
            exports: vec![(
                callable.ffi_func.0.as_str(),
                callable
                    .async_data
                    .as_ref()
                    .map(|a| a.ffi_rust_future_poll.0.as_str()),
            )],
        }
    }

    fn type_(
        name: &'a str,
        constructors: &'a [general::Constructor],
        methods: &'a [general::Method],
        traits: &'a general::UniffiTraitMethods,
        has_callbacks: bool,
    ) -> Self {
        let trait_methods = [
            &traits.display_fmt,
            &traits.debug_fmt,
            &traits.eq_eq,
            &traits.eq_ne,
            &traits.hash_hash,
            &traits.ord_cmp,
        ];
        let exports = constructors
            .iter()
            .map(|c| &c.callable)
            .chain(methods.iter().map(|m| &m.callable))
            .chain(
                trait_methods
                    .into_iter()
                    .filter_map(|m| m.as_ref())
                    .map(|m| &m.callable),
            )
            .map(|c| {
                (
                    c.ffi_func.0.as_str(),
                    c.async_data
                        .as_ref()
                        .map(|a| a.ffi_rust_future_poll.0.as_str()),
                )
            })
            .collect();
        let members = constructors
            .iter()
            .map(|c| {
                let name = if matches!(
                    c.callable.kind,
                    general::CallableKind::Constructor { primary: true, .. }
                ) {
                    "constructor"
                } else {
                    &c.callable.name
                };
                Self::callable(name, &c.callable, has_callbacks)
            })
            .chain(
                methods
                    .iter()
                    .map(|m| Self::callable(&m.callable.name, &m.callable, has_callbacks)),
            )
            .collect();
        Self {
            name,
            has_callbacks,
            members,
            exports,
        }
    }
}

/// Resolve the same public callable allowlist for both the Rust and TS generators.
/// Never select clone/free, checksums, allocation, registration or vtable entries.
pub(super) fn resolve(
    selection: &JspiSelection,
    namespace: &general::Namespace,
    is_web: bool,
) -> Result<HashSet<String>> {
    select(selection, &namespace_targets(namespace), is_web)
}

fn namespace_targets(namespace: &general::Namespace) -> Vec<Target<'_>> {
    let mut targets: Vec<_> = namespace
        .functions
        .iter()
        .map(|f| Target::callable(&f.callable.name, &f.callable, false))
        .collect();
    for ty in &namespace.type_definitions {
        let target = match ty {
            general::TypeDefinition::Interface(i) => Target::type_(
                &i.name,
                &i.constructors,
                &i.methods,
                &i.uniffi_trait_methods,
                i.imp.has_callback_interface(),
            ),
            general::TypeDefinition::Record(r) => Target::type_(
                &r.name,
                &r.constructors,
                &r.methods,
                &r.uniffi_trait_methods,
                false,
            ),
            general::TypeDefinition::Enum(e) => Target::type_(
                &e.name,
                &e.constructors,
                &e.methods,
                &e.uniffi_trait_methods,
                false,
            ),
            general::TypeDefinition::CallbackInterface(c) => Target {
                name: &c.name,
                has_callbacks: true,
                members: vec![],
                exports: vec![],
            },
            _ => continue,
        };
        targets.push(target);
    }
    targets
}

fn select(
    selection: &JspiSelection,
    targets: &[Target<'_>],
    is_web: bool,
) -> Result<HashSet<String>> {
    if !is_web {
        return Ok(HashSet::new());
    }
    let mut selected = Vec::new();
    let mut automatic_exports = Vec::new();
    let mut excluded = Vec::new();
    match selection {
        JspiSelection::All(true) => selected.extend(targets.iter().filter(|t| !t.has_callbacks)),
        JspiSelection::All(false) => {}
        JspiSelection::Named(names) => select_named(names, targets, &mut selected)?,
        JspiSelection::Options(options) => {
            if !options.rust_async && options.include.is_empty() && !options.exclude.is_empty() {
                selected.extend(targets.iter().filter(|t| !t.has_callbacks));
            } else if options.rust_async {
                automatic_exports.extend(rust_async_exports(targets));
            }
            select_named(&options.include, targets, &mut selected)?;
            select_named(&options.exclude, targets, &mut excluded)?;
        }
    }
    let excluded_exports: HashSet<_> = excluded
        .into_iter()
        .flat_map(|target| target.exports.iter().copied())
        .collect();
    Ok(selected
        .into_iter()
        .flat_map(|t| &t.exports)
        .copied()
        .chain(automatic_exports)
        .filter(|export| !excluded_exports.contains(export))
        .flat_map(|(symbol, poll)| {
            std::iter::once(symbol.to_owned()).chain(poll.map(|name| format!("{name}_jspi")))
        })
        .collect())
}

fn rust_async_exports<'a>(targets: &'a [Target<'a>]) -> Vec<(&'a str, Option<&'a str>)> {
    let mut exports = Vec::new();
    for target in targets {
        let callables = if target.members.is_empty() {
            std::slice::from_ref(target)
        } else {
            target.members.as_slice()
        };
        exports.extend(
            callables
                .iter()
                .filter(|callable| !callable.has_callbacks)
                .flat_map(|callable| callable.exports.iter())
                .filter(|(_, poll)| poll.is_some())
                .copied(),
        );
    }
    exports
}

fn select_named<'a>(
    names: &[String],
    targets: &'a [Target<'a>],
    selected: &mut Vec<&'a Target<'a>>,
) -> Result<()> {
    for name in names {
        let parts: Vec<_> = name.split('.').collect();
        ensure!(parts.len() <= 2 && parts.iter().all(|p| !p.trim().is_empty()),
                    "invalid jspi selector `{name}`: expected a function, type, or Type.member (Type.constructor for the primary constructor)");
        let owners: Vec<_> = targets
            .iter()
            .filter(|t| normalized_eq(t.name, parts[0]))
            .collect();
        ensure!(!owners.is_empty(), "unknown jspi selection `{name}`: no top-level function or object/record/enum type matches `{}`", parts[0]);
        // A function and a data-only type may normalize to the same public name
        // (for example `load_settings` and `LoadSettings`).  A type with no
        // callable exports cannot affect JSPI selection, so let the sole
        // callable owner win.  Multiple callable owners remain ambiguous.
        let callable_owners: Vec<_> = owners
            .iter()
            .copied()
            .filter(|target| !target.exports.is_empty())
            .collect();
        let owners = if callable_owners.len() == 1 {
            callable_owners
        } else {
            owners
        };
        ensure!(
            owners.len() == 1,
            "ambiguous jspi selection `{name}`: multiple top-level functions or types match `{}`",
            parts[0]
        );
        let mut target = owners[0];
        if parts.len() == 2 {
            let members: Vec<_> = target
                .members
                .iter()
                .filter(|m| normalized_eq(m.name, parts[1]))
                .collect();
            ensure!(!members.is_empty(), "unknown jspi selection `{name}`: no method or constructor matches `{}` on `{}` (use Type.constructor for the primary constructor)", parts[1], parts[0]);
            ensure!(members.len() == 1, "ambiguous jspi selection `{name}`: multiple methods or constructors match after name normalization");
            target = members[0];
        }
        ensure!(!target.has_callbacks,
                    "jspi selection `{name}` is callback-capable or has an unsupported ABI; suspending inbound callback interfaces is not supported");
        selected.push(target);
    }
    Ok(())
}

fn normalized_eq(left: &str, right: &str) -> bool {
    left.to_upper_camel_case() == right.to_upper_camel_case()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uniffi_bindgen::pipeline::initial;

    fn targets() -> Vec<Target<'static>> {
        vec![
            Target {
                name: "compute_value",
                has_callbacks: false,
                members: vec![],
                exports: vec![("ffi_compute", None)],
            },
            Target {
                name: "future",
                has_callbacks: false,
                members: vec![],
                exports: vec![("ffi_future", Some("ffi_poll_u32"))],
            },
            Target {
                name: "Processor",
                has_callbacks: false,
                members: vec![],
                exports: vec![
                    ("ffi_new", None),
                    ("ffi_method", None),
                    ("ffi_display", None),
                    ("ffi_async_method", Some("ffi_poll_u32")),
                ],
            },
            Target {
                name: "Listener",
                has_callbacks: true,
                members: vec![],
                exports: vec![("ffi_callback", None)],
            },
        ]
    }

    fn named(name: &str) -> JspiSelection {
        JspiSelection::Named(vec![name.into()])
    }

    fn member(
        name: &'static str,
        symbol: &'static str,
        poll: Option<&'static str>,
    ) -> Target<'static> {
        Target {
            name,
            has_callbacks: false,
            members: vec![],
            exports: vec![(symbol, poll)],
        }
    }

    #[test]
    fn granular_selection_normalizes_each_component_and_deduplicates_overlaps() {
        let mut targets = targets();
        targets[2].members = vec![
            member("constructor", "ffi_new", None),
            member("method", "ffi_method", None),
            member("async_method", "ffi_async_method", Some("ffi_poll_u32")),
        ];
        for wasm2 in [false, true] {
            let resolver = |s: &JspiSelection, t: &[Target<'_>]| {
                if wasm2 {
                    select_wasm2(s, t)
                } else {
                    select(s, t, true)
                }
            };
            assert_eq!(
                resolver(&named("processor.asyncMethod"), &targets).unwrap(),
                ["ffi_async_method", "ffi_poll_u32_jspi"]
                    .map(str::to_owned)
                    .into()
            );
            assert_eq!(
                resolver(&named("Processor.constructor"), &targets).unwrap(),
                ["ffi_new".to_owned()].into()
            );
            let overlap = JspiSelection::Named(vec![
                "Processor.asyncMethod".into(),
                "Processor".into(),
                "Processor.constructor".into(),
                "processor.async_method".into(),
            ]);
            assert_eq!(
                resolver(&overlap, &targets).unwrap(),
                resolver(&named("Processor"), &targets).unwrap()
            );
        }
        assert!(select(&named("Processor.asyncMethod"), &targets, false)
            .unwrap()
            .is_empty());
        // Member eligibility remains independent of its siblings on wasm2.
        targets[2].has_callbacks = true;
        assert!(select_wasm2(&named("Processor"), &targets).is_err());
        assert!(select_wasm2(&named("Processor.asyncMethod"), &targets).is_ok());
        targets[2].members[2].has_callbacks = true;
        assert!(select_wasm2(&named("Processor.asyncMethod"), &targets).is_err());
    }

    #[test]
    fn malformed_unknown_and_ambiguous_members_are_errors() {
        let mut targets = targets();
        targets[2].members = vec![
            member("from_label", "ffi_a", None),
            member("fromLabel", "ffi_b", None),
        ];
        for (selector, expected) in [
            ("Processor.fromLabel", "ambiguous"),
            ("Processor.missing", "unknown"),
            ("Processor.free", "unknown"),
            ("Processor.clone", "unknown"),
            ("computeValue.member", "unknown"),
            ("Processor.", "invalid"),
            (".constructor", "invalid"),
            ("Processor.member.extra", "invalid"),
            ("", "invalid"),
        ] {
            let error = select(&named(selector), &targets, true)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(expected) && error.contains(selector),
                "{error}"
            );
        }
        targets.push(member("processor", "ffi_collision", None));
        for selector in ["Processor", "Processor.fromLabel"] {
            assert!(select(&named(selector), &targets, true)
                .unwrap_err()
                .to_string()
                .contains("ambiguous"));
        }
    }

    #[test]
    fn a_zero_export_type_does_not_make_a_callable_name_ambiguous() {
        let mut targets = vec![
            member("load_settings", "ffi_load_settings", None),
            Target {
                name: "LoadSettings",
                has_callbacks: false,
                members: vec![],
                exports: vec![],
            },
        ];

        assert_eq!(
            select(&named("loadSettings"), &targets, true).unwrap(),
            ["ffi_load_settings".to_owned()].into()
        );
        targets.push(member("loadSettings", "ffi_other_load_settings", None));
        assert!(select(&named("loadSettings"), &targets, true)
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
    }

    #[test]
    fn exclude_only_never_selects_generated_clone_or_free_exports() {
        let namespace = initial::Namespace {
            name: "selection_fixture".into(),
            crate_name: "selection_fixture".into(),
            config_toml: None,
            docstring: None,
            functions: vec![
                initial::Function {
                    name: "load_settings".into(),
                    is_async: false,
                    inputs: vec![],
                    return_type: None,
                    throws: None,
                    checksum: Some(1),
                    docstring: None,
                },
                initial::Function {
                    name: "skip_call".into(),
                    is_async: false,
                    inputs: vec![],
                    return_type: None,
                    throws: None,
                    checksum: Some(2),
                    docstring: None,
                },
            ],
            type_definitions: vec![
                initial::TypeDefinition::Record(initial::Record {
                    name: "LoadSettings".into(),
                    fields: vec![],
                    constructors: vec![],
                    methods: vec![],
                    uniffi_traits: vec![],
                    docstring: None,
                }),
                initial::TypeDefinition::Interface(initial::Interface {
                    name: "Resource".into(),
                    docstring: None,
                    constructors: vec![],
                    methods: vec![],
                    uniffi_traits: vec![],
                    trait_impls: vec![],
                    imp: initial::ObjectImpl::Struct,
                }),
            ],
        };
        let root = initial::Root {
            namespaces: [(namespace.name.clone(), namespace)].into_iter().collect(),
            cdylib: None,
        };
        let root = general::pipeline("react-native").execute(root).unwrap();
        let namespace = &root.namespaces["selection_fixture"];
        let selection = JspiSelection::Options(super::super::config::JspiOptions {
            rust_async: false,
            include: vec![],
            exclude: vec!["loadSettings".into()],
        });
        let selected = resolve(&selection, namespace, true).unwrap();

        let infrastructure: Vec<_> = namespace
            .ffi_definitions
            .iter()
            .filter_map(|definition| match definition {
                general::FfiDefinition::RustFunction(function)
                    if matches!(
                        function.kind,
                        general::FfiFunctionKind::ObjectClone
                            | general::FfiFunctionKind::ObjectFree
                    ) =>
                {
                    Some(function.name.0.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(infrastructure.len(), 2);
        assert!(infrastructure
            .iter()
            .all(|symbol| !selected.contains(*symbol)));
        assert_eq!(selected.len(), 1);
        assert!(selected.iter().any(|symbol| symbol.contains("skip_call")));

        #[cfg(feature = "wasm")]
        {
            let mut config = super::super::Config::default();
            config.resolved_jspi_exports = selected;
            let module = super::super::ffi_module_player::PlayerFfiModule::from_general(
                namespace,
                &config,
                &crate::AbiFlavor::Wasm2,
                namespace.crate_name.clone(),
                None,
            );
            assert!(module.functions.iter().all(|function| {
                !infrastructure.contains(&function.name.as_str()) || !function.jspi
            }));
        }
    }

    #[test]
    fn wasm2_selects_async_and_type_callables_and_rejects_callbacks() {
        let mut top = targets();
        top.truncate(2); // the namespace contributes functions only
        assert_eq!(
            select_wasm2(&JspiSelection::All(true), &top).unwrap(),
            ["ffi_compute", "ffi_future", "ffi_poll_u32_jspi"]
                .map(str::to_owned)
                .into()
        );
        assert_eq!(
            select_wasm2(&named("computeValue"), &top).unwrap(),
            ["ffi_compute".to_owned()].into()
        );
        assert_eq!(
            select_wasm2(&named("future"), &top).unwrap(),
            ["ffi_future", "ffi_poll_u32_jspi"]
                .map(str::to_owned)
                .into()
        );
        assert_eq!(
            select_wasm2(&named("Processor"), &targets()).unwrap(),
            [
                "ffi_new",
                "ffi_method",
                "ffi_display",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );
        assert!(select_wasm2(&named("Listener"), &targets()).is_err());
        top.truncate(1);
        top[0].has_callbacks = true;
        assert!(select_wasm2(&JspiSelection::All(true), &top)
            .unwrap()
            .is_empty());
        assert!(select_wasm2(&named("computeValue"), &top).is_err());
    }

    #[test]
    fn selection_includes_dedicated_async_poll_adapters() {
        assert_eq!(
            select(&named("processor"), &targets(), true).unwrap(),
            [
                "ffi_new",
                "ffi_method",
                "ffi_display",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );
        assert_eq!(
            select(&named("future"), &targets(), true).unwrap(),
            ["ffi_future".to_owned(), "ffi_poll_u32_jspi".to_owned()].into()
        );
        assert_eq!(
            select(&named("computeValue"), &targets(), true).unwrap(),
            ["ffi_compute".to_owned()].into()
        );
        assert_eq!(
            select(&JspiSelection::All(true), &targets(), true).unwrap(),
            [
                "ffi_compute",
                "ffi_new",
                "ffi_method",
                "ffi_display",
                "ffi_future",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );
    }

    #[test]
    fn invalid_selection_is_an_error_and_unsupported_backends_ignore_jspi() {
        for (name, message) in [
            ("Listener", "callback-capable"),
            ("Processor.new", "primary constructor"),
            ("typo", "top-level function"),
        ] {
            assert!(select(&named(name), &targets(), true)
                .unwrap_err()
                .to_string()
                .contains(message));
        }
        assert!(select(&JspiSelection::All(true), &targets(), false)
            .unwrap()
            .is_empty());
        assert!(select(&named("typo"), &targets(), false)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn disabled_and_force_async_only_do_not_select_jspi_exports() {
        for setting in ["", "forceAsync = true", "jspi = false", "jspi = []"] {
            let config: super::super::Config = toml::from_str(setting).unwrap();
            assert!(select(&config.jspi, &targets(), false).unwrap().is_empty());
        }
    }

    #[test]
    fn rust_async_selects_only_async_callables_and_unions_includes() {
        let automatic = JspiSelection::Options(super::super::config::JspiOptions {
            rust_async: true,
            include: vec![],
            exclude: vec![],
        });
        assert_eq!(
            select(&automatic, &targets(), true).unwrap(),
            ["ffi_future", "ffi_async_method", "ffi_poll_u32_jspi"]
                .map(str::to_owned)
                .into()
        );

        let with_sync = JspiSelection::Options(super::super::config::JspiOptions {
            rust_async: true,
            include: vec!["computeValue".into()],
            exclude: vec![],
        });
        assert_eq!(
            select(&with_sync, &targets(), true).unwrap(),
            [
                "ffi_compute",
                "ffi_future",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );
        assert!(select(&automatic, &targets(), false).unwrap().is_empty());
    }

    #[test]
    fn exclusions_subtract_from_all_or_an_explicit_selection() {
        let mut targets = targets();
        targets[2].members = vec![
            member("constructor", "ffi_new", None),
            member("method", "ffi_method", None),
            member("async_method", "ffi_async_method", Some("ffi_poll_u32")),
        ];
        let all_except_constructor = JspiSelection::Options(super::super::config::JspiOptions {
            rust_async: false,
            include: vec![],
            exclude: vec!["Processor.constructor".into()],
        });
        assert_eq!(
            select(&all_except_constructor, &targets, true).unwrap(),
            [
                "ffi_compute",
                "ffi_future",
                "ffi_method",
                "ffi_display",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );

        let included_type_except_method =
            JspiSelection::Options(super::super::config::JspiOptions {
                rust_async: false,
                include: vec!["Processor".into()],
                exclude: vec!["Processor.method".into()],
            });
        assert_eq!(
            select(&included_type_except_method, &targets, true).unwrap(),
            [
                "ffi_new",
                "ffi_display",
                "ffi_async_method",
                "ffi_poll_u32_jspi"
            ]
            .map(str::to_owned)
            .into()
        );
    }
}
