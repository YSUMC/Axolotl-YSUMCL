//! Brand policy is selected by the shell at runtime. Both distributions use the
//! same compiled library and never accept a manifest endpoint from user input.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandPolicy {
    pub id: String,
    pub identifier: String,
    pub api_base: String,
    pub login_mode: String,
    pub scheme: String,
    pub short_name: String,
}

static BRANDS: OnceLock<BTreeMap<String, BrandPolicy>> = OnceLock::new();
static ACTIVE: OnceLock<&'static str> = OnceLock::new();
static APP_VERSION: OnceLock<String> = OnceLock::new();

fn brands() -> &'static BTreeMap<String, BrandPolicy> {
    BRANDS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../brands/catalog.json"))
            .expect("Invalid bundled launcher brand configuration")
    })
}

pub fn initialize(identifier: &str, version: &str) {
    let brand = if identifier == brands()["mutmc"].identifier {
        "mutmc"
    } else {
        "ysumc"
    };
    let _ = ACTIVE.set(brand);
    let _ = APP_VERSION.set(version.to_owned());
}

pub fn app_version() -> &'static str {
    APP_VERSION
        .get()
        .map(String::as_str)
        .unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn active() -> &'static BrandPolicy {
    &brands()[ACTIVE.get().copied().unwrap_or("ysumc")]
}

pub fn manifest_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| format!("{}/manifest", active().api_base))
}

pub fn accepts_account(offline: bool, yggdrasil_root: Option<&str>) -> bool {
    if offline {
        return false;
    }
    if active().login_mode == "microsoft" {
        yggdrasil_root.is_none()
    } else {
        yggdrasil_root.is_some_and(|root| {
            root.trim_end_matches('/') == crate::ysumc::YGGDRASIL_API_ROOT
        })
    }
}
