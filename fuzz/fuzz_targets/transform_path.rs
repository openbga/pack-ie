//! Path parsing in every syntax on arbitrary text, then the accessors and `resolved`, and the
//! schema checks (read and write) that the validator runs on parsed paths.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_transform::{Path, Schema, Syntax};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let schemas = [
        Schema::hl7(),
        Schema::astm(),
        Schema::au(),
        Schema::delimited(["sample_id", "result", "c0"]),
    ];
    for syntax in [Syntax::Hl7, Syntax::Astm, Syntax::Au, Syntax::Delimited] {
        let Ok(path) = Path::parse(syntax, text) else {
            continue;
        };
        assert_eq!(path.as_str(), text);
        let _ = (path.syntax(), path.scope(), path.record(), path.field());
        let _ = path.resolved("OBX", 3);
        let _ = path.resolved("", usize::MAX);
        for schema in &schemas {
            let _ = schema.check(&path, false);
            let _ = schema.check(&path, true);
        }
    }
    for schema in &schemas {
        let _ = schema.parse(text);
        let _ = schema.check_record(text, false);
        let _ = schema.check_record(text, true);
    }
});
