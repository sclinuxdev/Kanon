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
