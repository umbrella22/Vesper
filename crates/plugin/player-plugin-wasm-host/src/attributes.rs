use std::collections::{BTreeMap, btree_map::Entry};

use player_plugin::{
    MAX_PLUGIN_ATTRIBUTE_KEY_BYTES, MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES, MAX_PLUGIN_ATTRIBUTES,
};

/// Decode either world's attribute records without erasing duplicate keys or
/// changing valid text. WIT-generated record types stay local to their world.
pub(crate) fn decode_attributes(
    attributes: impl ExactSizeIterator<Item = (String, String)>,
) -> Result<BTreeMap<String, String>, String> {
    if attributes.len() > MAX_PLUGIN_ATTRIBUTES {
        return Err(format!(
            "plugin attributes exceed the {MAX_PLUGIN_ATTRIBUTES}-entry protocol limit"
        ));
    }
    let mut result = BTreeMap::new();
    for (key, value) in attributes {
        if key.is_empty() || key.len() > MAX_PLUGIN_ATTRIBUTE_KEY_BYTES {
            return Err(format!(
                "attribute.key must contain 1 to {MAX_PLUGIN_ATTRIBUTE_KEY_BYTES} UTF-8 bytes"
            ));
        }
        if value.is_empty() || value.len() > MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES {
            return Err(format!(
                "attribute.value must contain 1 to {MAX_PLUGIN_ATTRIBUTE_VALUE_BYTES} UTF-8 bytes"
            ));
        }
        match result.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(entry) => {
                return Err(format!(
                    "plugin returned duplicate attribute key '{}'",
                    entry.key()
                ));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_whitespace_and_multibyte_text_are_preserved() {
        let entries = [(" 路径 ".to_owned(), " 値\t ".to_owned())];
        let decoded = decode_attributes(entries.clone().into_iter()).expect("valid attributes");
        assert_eq!(decoded, BTreeMap::from(entries));
    }

    #[test]
    fn count_and_utf8_byte_limits_are_enforced_before_map_conversion() {
        let entries = (0..MAX_PLUGIN_ATTRIBUTES + 1).map(|n| (n.to_string(), "v".to_owned()));
        assert!(decode_attributes(entries).is_err());
        let long_key = "界".repeat(MAX_PLUGIN_ATTRIBUTE_KEY_BYTES / 3 + 1);
        assert!(decode_attributes([(long_key, "value".to_owned())].into_iter()).is_err());
        assert!(decode_attributes([("key".to_owned(), String::new())].into_iter()).is_err());
    }
}
