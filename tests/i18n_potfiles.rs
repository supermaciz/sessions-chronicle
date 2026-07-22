#[test]
fn potfiles_includes_rust_files_with_gettext_strings() {
    let potfiles = include_str!("../po/POTFILES.in");
    for path in ["src/ui/modals/shortcuts.rs", "src/ui/sort_pill.rs"] {
        assert!(
            potfiles.lines().any(|line| line.trim() == path),
            "po/POTFILES.in must include {path} for gettext extraction"
        );
    }
}
