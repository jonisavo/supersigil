//! Property-based tests for `split_list_attribute`.

use proptest::prelude::*;
use supersigil_core::split_list_attribute;

/// Generate a non-empty, non-comma, non-whitespace-only item suitable for
/// comma-separated lists.
fn arb_item() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_/\\-]{1,20}"
}

/// Generate a valid comma-separated string from 1..=5 items, with optional
/// surrounding whitespace per item.
fn arb_valid_comma_separated() -> impl Strategy<Value = (String, Vec<String>)> {
    prop::collection::vec(arb_item(), 1..=5).prop_flat_map(|items| {
        let n = items.len();
        let items_clone = items.clone();
        prop::collection::vec("[ ]{0,3}", n).prop_map(move |pads| {
            let raw = items_clone
                .iter()
                .zip(pads.iter())
                .map(|(item, pad)| format!("{pad}{item}{pad}"))
                .collect::<Vec<_>>()
                .join(",");
            (raw, items_clone.clone())
        })
    })
}

/// Generate a string with a trailing comma (should be rejected).
fn arb_trailing_comma() -> impl Strategy<Value = String> {
    arb_valid_comma_separated().prop_map(|(raw, _)| format!("{raw},"))
}

/// Generate a string with consecutive commas (should be rejected).
fn arb_consecutive_commas() -> impl Strategy<Value = String> {
    arb_valid_comma_separated().prop_map(|(raw, _)| {
        // Insert an extra comma after the first comma
        if let Some(pos) = raw.find(',') {
            let mut s = raw.clone();
            s.insert(pos + 1, ',');
            s
        } else {
            // Single item — just append ",,"
            format!("{raw},,")
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Feature: parser-and-config, Property 15: List splitting produces trimmed non-empty items
    #[test]
    fn valid_items_are_trimmed_and_nonempty((raw, expected) in arb_valid_comma_separated()) {
        let result = split_list_attribute(&raw).unwrap();
        // Every output item is non-empty and trimmed
        for item in &result {
            prop_assert!(!item.is_empty(), "item should not be empty");
            prop_assert_eq!(*item, item.trim(), "item should be trimmed");
        }
        // Output matches expected items
        prop_assert_eq!(result, expected);
    }

    #[test]
    fn trailing_comma_rejected(raw in arb_trailing_comma()) {
        let result = split_list_attribute(&raw);
        prop_assert!(result.is_err(), "trailing comma should be rejected: {raw}");
    }

    #[test]
    fn consecutive_commas_rejected(raw in arb_consecutive_commas()) {
        let result = split_list_attribute(&raw);
        prop_assert!(result.is_err(), "consecutive commas should be rejected: {raw}");
    }

    #[test]
    fn empty_input_rejected(raw in "[ ]*") {
        // Empty or whitespace-only input: if truly empty, error; if whitespace-only,
        // it's a single item that trims to empty, so also error.
        if raw.is_empty() {
            let result = split_list_attribute(&raw);
            prop_assert!(result.is_err(), "empty input should be rejected");
        } else {
            // Whitespace-only: split produces one item that trims to empty
            // Our implementation splits on comma first, so " " -> [""] after trim -> error
            // Actually " " splits to [" "] -> trim -> [""] -> but wait, " ".trim() = ""
            // which is empty, so it should error. Let's verify:
            // split_list_attribute splits on ',', trims, rejects empty.
            // " " has no comma, so items = [" "], trimmed = [""], which is empty -> error.
            let result = split_list_attribute(&raw);
            prop_assert!(result.is_err(), "whitespace-only input should be rejected: '{raw}'");
        }
    }
}
