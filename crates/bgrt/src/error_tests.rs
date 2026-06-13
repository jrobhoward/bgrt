//! Tests for the error types.
#![allow(non_snake_case)]

use super::Error;

#[test]
fn backend_error____display____prefixes_message() {
    let e = Error::Backend("boom".to_owned());
    assert_eq!(e.to_string(), "failed to apply energy qos: boom");
}
