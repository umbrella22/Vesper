#![cfg(vesper_source_checkout)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use syn::{Expr, ExprLit, Item, Lit};

#[derive(Debug, Deserialize)]
struct BridgeManifest {
    private_declarations: Vec<BridgeDeclaration>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum BridgeDeclaration {
    #[serde(rename = "enum")]
    Enum {
        name: String,
        variants: Vec<BridgeVariant>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct BridgeVariant {
    name: String,
    value: i64,
}

#[test]
fn ios_bridge_manifest_enums_match_rust_ffi_values() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let manifest_path = root.join("scripts/ios/bridge-shim/manifest.json");
    let rust_types_path = root.join("crates/ffi/player-ffi-ios/src/types.rs");

    let manifest: BridgeManifest = serde_json::from_str(
        &fs::read_to_string(&manifest_path).expect("read iOS bridge manifest"),
    )
    .expect("parse iOS bridge manifest");
    let syntax =
        syn::parse_file(&fs::read_to_string(&rust_types_path).expect("read Rust FFI types"))
            .expect("parse Rust iOS FFI types");

    let rust_enums = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(item) if item.ident.to_string().starts_with("PlayerFfi") => Some((
                item.ident.to_string(),
                enum_variants(&item.ident.to_string(), item),
            )),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    for declaration in manifest.private_declarations {
        let BridgeDeclaration::Enum { name, variants } = declaration else {
            continue;
        };
        let rust_variants = rust_enums
            .get(&name)
            .unwrap_or_else(|| panic!("bridge enum {name} is missing from Rust FFI types"));
        let manifest_variants = variants
            .into_iter()
            .map(|variant| (variant.name, variant.value))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            &manifest_variants, rust_variants,
            "bridge enum {name} differs from Rust FFI enum"
        );
    }
}

fn enum_variants(enum_name: &str, item: &syn::ItemEnum) -> BTreeMap<String, i64> {
    item.variants
        .iter()
        .map(|variant| {
            let value = variant
                .discriminant
                .as_ref()
                .and_then(|(_, expression)| match expression {
                    Expr::Lit(ExprLit {
                        lit: Lit::Int(value),
                        ..
                    }) => value.base10_parse().ok(),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "Rust FFI enum variant {} lacks an integer value",
                        variant.ident
                    )
                });
            (format!("{enum_name}{}", variant.ident), value)
        })
        .collect()
}
