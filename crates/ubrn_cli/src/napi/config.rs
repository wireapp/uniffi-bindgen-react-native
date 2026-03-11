/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/
 */
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NapiConfig {
    #[serde(default)]
    pub(crate) features: Option<Vec<String>>,

    #[serde(default)]
    pub(crate) default_features: Option<bool>,
}

impl Default for NapiConfig {
    fn default() -> Self {
        Self {
            features: None,
            default_features: None,
        }
    }
}
