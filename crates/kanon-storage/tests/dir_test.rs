//! Integration tests for PluginDataDir directory isolation.

use kanon_storage::PluginDataDir;
use tempfile::tempdir;

#[test]
fn test_plugin_data_dir_resolution() {
    let tmp = tempdir().expect("Failed to create temporary directory");
    let manager = PluginDataDir::new(tmp.path());

    let plugin_id = "org.kanon.plugin.weather";
    let plugin_dir = manager
        .resolve_plugin_dir(plugin_id)
        .expect("Failed to resolve plugin directory");

    assert!(plugin_dir.exists());
    assert!(plugin_dir.is_dir());
    assert_eq!(plugin_dir, tmp.path().join(plugin_id));

    let db_path = manager
        .resolve_db_path(plugin_id, "memory.db")
        .expect("Failed to resolve db path");
    assert_eq!(db_path, plugin_dir.join("memory.db"));
}

#[test]
fn test_plugin_data_dir_rejects_traversal_attacks() {
    let tmp = tempdir().expect("Failed to create temporary directory");
    let manager = PluginDataDir::new(tmp.path());

    let invalid_ids = [
        "../escape",
        "..",
        ".",
        "",
        "/etc/passwd",
        "foo/bar",
        "foo\\bar",
        "foo\0bar",
        "   ",
        "foo\nbar",
    ];

    for invalid_id in invalid_ids {
        let res = manager.resolve_plugin_dir(invalid_id);
        assert!(
            res.is_err(),
            "Expected plugin_id {:?} to be rejected, but got Ok({:?})",
            invalid_id,
            res.unwrap()
        );
        let err = res.unwrap_err();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::InvalidInput,
            "Expected InvalidInput error kind for {:?}",
            invalid_id
        );
    }
}
