use std::path::Path;

#[test]
fn potfiles_lists_only_existing_sources_and_external_open_translations() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let potfiles = include_str!("../po/POTFILES.in");
    let entries: Vec<&str> = potfiles
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    for entry in &entries {
        assert!(
            manifest_dir.join(entry).is_file(),
            "po/POTFILES.in references missing path {entry}"
        );
    }

    for required in [
        "src/app/handlers/sessions.rs",
        "src/ui/modals/shortcuts.rs",
        "src/ui/sort_pill.rs",
    ] {
        assert!(
            entries.contains(&required),
            "po/POTFILES.in must include {required} for gettext extraction"
        );
    }
}
