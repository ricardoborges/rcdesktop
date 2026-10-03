use rcdesktop::settings::{autostart, UserSettings};

#[test]
fn settings_default_to_tray_friendly_values_and_tolerate_missing_keys() {
    assert_eq!(UserSettings::default(), UserSettings { start_minimized: true, close_to_tray: true });
    let partial: UserSettings = serde_json::from_str(r#"{"close_to_tray": false}"#).unwrap();
    assert!(partial.start_minimized);
    assert!(!partial.close_to_tray);
}

/// Writes and removes the real HKCU Run entry: `cargo test -- --ignored`.
/// Skipped when autostart is already on, so a real setup isn't clobbered.
#[test]
#[ignore]
fn autostart_entry_round_trip() {
    if autostart::is_enabled() {
        eprintln!("autostart already enabled; skipping");
        return;
    }
    autostart::set_enabled(true).unwrap();
    assert!(autostart::is_enabled());
    autostart::set_enabled(false).unwrap();
    assert!(!autostart::is_enabled());
    // Disabling twice is fine
    autostart::set_enabled(false).unwrap();
}
