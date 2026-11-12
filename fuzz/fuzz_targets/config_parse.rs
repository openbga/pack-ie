//! RuntimeConfig::parse on arbitrary JSON text: Ok or a ValidationErrors, never a panic. A
//! document that is accepted must serialize and parse again (to a configuration that is then
//! stable), and the other entry points must agree with `parse` on whether it is accepted.
//! (`from_value` is checked in one direction only: a `Value` has already dropped duplicate
//! keys, which the text parser rejects, and `parse` ignores text after the document.)
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_config::RuntimeConfig;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let parsed = RuntimeConfig::parse(text);
    if let Err(errors) = &parsed {
        let _ = errors.to_string();
    }
    let with_warnings = RuntimeConfig::parse_with_warnings(text);
    assert_eq!(parsed.is_ok(), with_warnings.is_ok());
    if let (Ok(_), Ok(value)) = (&parsed, serde_json::from_str::<serde_json::Value>(text)) {
        assert!(RuntimeConfig::from_value(value).is_ok());
    }
    if let Ok(config) = parsed {
        // Decimals with more than ~17 digits are rounded through f64 on the first save, so the
        // first reparse may differ from the input; from there on it must be a fixed point.
        let once = reparse(&config);
        assert_eq!(reparse(&once), once);
    }
});

fn reparse(config: &RuntimeConfig) -> RuntimeConfig {
    let json = config
        .to_pretty_json()
        .expect("an accepted config serializes");
    RuntimeConfig::parse(&json)
        .unwrap_or_else(|errors| panic!("an accepted config does not parse again: {errors}"))
}
