#![expect(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    reason = "Generated C API bindings preserve upstream identifiers"
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
