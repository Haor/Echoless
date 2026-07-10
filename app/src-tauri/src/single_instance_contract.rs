#[test]
fn single_instance_plugin_is_registered_first_and_focuses_main_window() {
    let source = include_str!("lib.rs");
    let first_plugin = source.find(".plugin(").expect("builder has no plugins");
    let single_instance = source
        .find(".plugin(tauri_plugin_single_instance::init")
        .expect("single-instance plugin is not registered");

    assert_eq!(
        first_plugin, single_instance,
        "single-instance must be the first registered Tauri plugin"
    );
    assert!(
        source.contains("show_main_window(app)"),
        "second-instance callback must restore and focus the main window"
    );
    let logging_init = source
        .find("logging::init")
        .expect("desktop logging is not initialized");
    assert!(
        logging_init > single_instance,
        "shared log files must not be opened before the single-instance gate"
    );
}
