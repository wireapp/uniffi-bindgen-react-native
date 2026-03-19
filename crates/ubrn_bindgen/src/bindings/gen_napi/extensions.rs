/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use std::collections::HashMap;

use extend::ext;
use heck::ToSnakeCase;
use syn::Ident;
use uniffi_bindgen::{
    interface::{FfiCallbackFunction, FfiDefinition, FfiField, FfiStruct, FfiType},
    ComponentInterface,
};

use super::util::{ident, snake_case_ident};
use crate::bindings::extensions::{FfiCallbackFunctionExt as _, FfiStructExt as _};

#[ext]
pub(super) impl ComponentInterface {
    fn has_callbacks(&self) -> bool {
        !self.callback_interface_definitions().is_empty()
            || self
                .object_definitions()
                .iter()
                .any(|object| object.has_callback_interface())
    }

    fn has_async_calls(&self) -> bool {
        self.iter_callables().any(|callable| callable.is_async())
    }

    fn ffi_definitions2(&self) -> impl Iterator<Item = FfiDefinition2> {
        let has_async_callbacks = self.has_async_callback_interface_definition();
        let has_callbacks = self.has_callbacks();
        let has_async_calls = self.has_async_calls();
        ffi_definitions2(
            self.ffi_definitions(),
            has_async_calls,
            has_callbacks,
            has_async_callbacks,
        )
    }
}

enum CallbackRole {
    Free,
    Clone,
    UserMethod,
    FutureInfra,
    Continuation,
    FunctionLiteral,
}

fn classify_callback(callback: &FfiCallbackFunction) -> CallbackRole {
    if callback.is_free_callback() {
        CallbackRole::Free
    } else if callback.is_clone_callback() {
        CallbackRole::Clone
    } else if callback.is_user_callback() {
        CallbackRole::UserMethod
    } else if callback.is_function_literal() {
        CallbackRole::FunctionLiteral
    } else if callback.is_continuation_callback() {
        CallbackRole::Continuation
    } else {
        CallbackRole::FutureInfra
    }
}

fn ffi_definitions2(
    definitions: impl Iterator<Item = FfiDefinition>,
    has_async_calls: bool,
    has_callbacks: bool,
    has_async_callbacks: bool,
) -> impl Iterator<Item = FfiDefinition2> {
    let mut callbacks = HashMap::new();
    let mut structs = HashMap::new();
    for definition in definitions {
        match definition {
            FfiDefinition::CallbackFunction(cb) => {
                callbacks.insert(cb.name().to_owned(), cb);
            }
            FfiDefinition::Struct(st) => {
                structs.insert(st.name().to_owned(), st);
            }
            _ => (),
        }
    }

    let mut definitions = Vec::new();

    for ffi_struct in structs.into_values() {
        if !has_async_callbacks && ffi_struct.is_foreign_future() {
            continue;
        }
        if !has_callbacks && ffi_struct.is_vtable() {
            continue;
        }

        let mut method_module_idents = HashMap::new();
        for field in ffi_struct.fields() {
            let FfiType::Callback(name) = &field.type_() else {
                continue;
            };
            let Some(callback) = callbacks.get(name) else {
                panic!("Missing callback. This is a bug in ubrn");
            };
            let module_ident = match classify_callback(callback) {
                CallbackRole::Free => {
                    let ident = callback.module_ident_free(&ffi_struct);
                    let callback = callback.clone();
                    let module_ident = ident.clone();
                    let cb = FfiCallbackFunction2 {
                        callback,
                        module_ident,
                    };
                    definitions.push(FfiDefinition2::CallbackFunction(cb));
                    ident
                }
                CallbackRole::Clone => {
                    let ident = callback.module_ident_clone(&ffi_struct);
                    let callback = callback.clone();
                    let module_ident = ident.clone();
                    let cb = FfiCallbackFunction2 {
                        callback,
                        module_ident,
                    };
                    definitions.push(FfiDefinition2::CallbackFunction(cb));
                    ident
                }
                _ => callback.module_ident(),
            };
            method_module_idents.insert(field.name().to_string(), module_ident);
        }

        definitions.push(FfiDefinition2::Struct(FfiStruct2 {
            ffi_struct,
            methods: method_module_idents,
        }));
    }

    for callback in callbacks.into_values() {
        match classify_callback(&callback) {
            CallbackRole::Free | CallbackRole::Clone => {
                continue;
            }
            CallbackRole::FutureInfra => {
                if !has_async_callbacks {
                    continue;
                }
            }
            CallbackRole::Continuation => {
                if !has_async_calls {
                    continue;
                }
            }
            CallbackRole::UserMethod => {
                if !has_callbacks {
                    continue;
                }
            }
            CallbackRole::FunctionLiteral => {
                if !has_async_callbacks {
                    continue;
                }
            }
        }

        let cb = FfiCallbackFunction2 {
            module_ident: callback.module_ident(),
            callback,
        };

        if cb.callback.is_function_literal() {
            definitions.push(FfiDefinition2::FunctionLiteral(cb));
        } else {
            definitions.push(FfiDefinition2::CallbackFunction(cb));
        }
    }

    definitions.into_iter()
}

pub(super) enum FfiDefinition2 {
    CallbackFunction(FfiCallbackFunction2),
    FunctionLiteral(FfiCallbackFunction2),
    Struct(FfiStruct2),
}

pub(super) struct FfiCallbackFunction2 {
    module_ident: Ident,
    callback: FfiCallbackFunction,
}

pub(super) struct FfiStruct2 {
    ffi_struct: FfiStruct,
    methods: HashMap<String, Ident>,
}

#[ext]
pub(super) impl FfiStruct {
    fn module_ident(&self) -> Ident {
        snake_case_ident(self.name())
    }
}

#[ext]
pub(super) impl FfiCallbackFunction {
    fn module_ident_free(&self, enclosing: &FfiStruct) -> Ident {
        ident(&format!("{}__free", enclosing.name().to_snake_case()))
    }

    fn module_ident_clone(&self, enclosing: &FfiStruct) -> Ident {
        ident(&format!("{}__clone", enclosing.name().to_snake_case()))
    }

    fn module_ident(&self) -> Ident {
        snake_case_ident(self.name())
    }

    fn is_clone_callback(&self) -> bool {
        self.name() == "CallbackInterfaceClone"
    }

    fn is_future_callback(&self) -> bool {
        self.name().starts_with("ForeignFuture") && self.name() != "ForeignFutureDroppedCallback"
    }
}

impl FfiCallbackFunction2 {
    pub(super) fn module_ident(&self) -> Ident {
        self.module_ident.clone()
    }

    pub(super) fn return_type(&self) -> Option<FfiType> {
        self.callback.arg_return_type()
    }

    pub(super) fn has_return_out_param(&self) -> bool {
        self.callback.has_return_out_param()
    }

    pub(super) fn callback(&self) -> &FfiCallbackFunction {
        &self.callback
    }
}

impl FfiStruct2 {
    pub(super) fn module_ident(&self) -> Ident {
        self.ffi_struct.module_ident()
    }

    pub(super) fn is_callback_method(&self, name: &str) -> bool {
        self.methods.contains_key(name)
    }

    pub(super) fn method_alias_ident(&self, name: &str) -> Ident {
        ident(&format!("method_{name}"))
    }

    pub(super) fn method_mod_ident(&self, name: &str) -> Ident {
        self.methods
            .get(name)
            .expect("Method not found. This is probably a ubrn bug.")
            .clone()
    }

    pub(super) fn method_names(&self) -> impl Iterator<Item = &str> {
        self.ffi_struct
            .fields()
            .iter()
            .filter(|f| matches!(f.type_(), FfiType::Callback(_)))
            .map(|f| f.name())
    }

    pub(super) fn fields(&self) -> impl Iterator<Item = &FfiField> {
        self.ffi_struct.fields().iter()
    }

    pub(super) fn is_passed_from_js_to_rust(&self) -> bool {
        self.ffi_struct.is_foreign_future()
    }
}
