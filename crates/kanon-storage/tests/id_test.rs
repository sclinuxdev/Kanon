//! Integration tests for strongly-typed PluginId.

use kanon_storage::{PluginId, PluginIdError};

#[test]
fn test_plugin_id_valid() {
    let valid_cases = [
        "plugin_a",
        "org.kanon.weather",
        "my-plugin-123",
        "custom_plugin",
        "plugin.v1.service",
    ];

    for case in valid_cases {
        let id: Result<PluginId, _> = case.parse();
        assert!(id.is_ok(), "Expected {case} to parse successfully");
        let id = id.unwrap();
        assert_eq!(id.as_str(), case);
        assert_eq!(id.to_string(), case);
    }
}

#[test]
fn test_plugin_id_invalid_cases() {
    assert_eq!("".parse::<PluginId>(), Err(PluginIdError::Empty));
    assert_eq!("   ".parse::<PluginId>(), Err(PluginIdError::Empty));
    assert_eq!(
        ".".parse::<PluginId>(),
        Err(PluginIdError::ReservedPathToken)
    );
    assert_eq!(
        "..".parse::<PluginId>(),
        Err(PluginIdError::ReservedPathToken)
    );
    assert_eq!(
        "a/b".parse::<PluginId>(),
        Err(PluginIdError::ContainsPathSeparator)
    );
    assert_eq!(
        "a\\b".parse::<PluginId>(),
        Err(PluginIdError::ContainsPathSeparator)
    );
    assert_eq!(
        "a\0b".parse::<PluginId>(),
        Err(PluginIdError::ContainsNullByte)
    );
    assert_eq!(
        "a\nb".parse::<PluginId>(),
        Err(PluginIdError::InvalidCharacters)
    );
    assert_eq!(
        " foo".parse::<PluginId>(),
        Err(PluginIdError::InvalidCharacters)
    );
    assert_eq!(
        "foo ".parse::<PluginId>(),
        Err(PluginIdError::InvalidCharacters)
    );
}

#[test]
fn test_plugin_id_serde() {
    let raw = "org.kanon.plugin";
    let id: PluginId = raw.parse().unwrap();

    let json = serde_json::to_string(&id).expect("Failed to serialize PluginId");
    assert_eq!(json, format!("\"{raw}\""));

    let deserialized: PluginId =
        serde_json::from_str(&json).expect("Failed to deserialize PluginId");
    assert_eq!(deserialized, id);

    let invalid_json = "\"../invalid\"";
    let err = serde_json::from_str::<PluginId>(invalid_json);
    assert!(err.is_err());
}
