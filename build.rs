fn main() {
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("io.github.supermaciz.sessionschronicle"),
        None::<&str>,
        None::<&str>,
        ["graph"],
    );
}
