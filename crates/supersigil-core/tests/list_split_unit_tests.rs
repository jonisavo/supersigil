//! Unit tests for `split_list_attribute`.

use supersigil_core::split_list_attribute;

#[test]
fn single_item() {
    let result = split_list_attribute("foo").unwrap();
    assert_eq!(result, vec!["foo"]);
}

#[test]
fn multiple_items() {
    let result = split_list_attribute("a, b, c").unwrap();
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn whitespace_trimming() {
    let result = split_list_attribute("  a , b  ,c  ").unwrap();
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn trailing_comma_is_error() {
    let result = split_list_attribute("a, b,");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.raw, "a, b,");
}

#[test]
fn consecutive_commas_is_error() {
    let result = split_list_attribute("a,,b");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.raw, "a,,b");
}

#[test]
fn empty_string_is_error() {
    let result = split_list_attribute("");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.raw, "");
}

#[test]
fn whitespace_only_items_is_error() {
    let result = split_list_attribute("a, , b");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.raw, "a, , b");
}
