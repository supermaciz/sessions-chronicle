#[test]
fn potfiles_includes_shortcuts_rust_file() {
    let potfiles = include_str!("../po/POTFILES.in");

    assert!(
        potfiles
            .lines()
            .any(|line| line.trim() == "src/ui/modals/shortcuts.rs"),
        "po/POTFILES.in must include src/ui/modals/shortcuts.rs for gettext extraction"
    );
}
