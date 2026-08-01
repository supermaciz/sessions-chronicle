#[rustfmt::skip]
mod config;
mod analytics_worker;
mod app;
#[allow(dead_code)]
mod database;
#[allow(dead_code)]
mod icon_names {
    pub use shipped::*;
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}
mod indexing_worker;
#[allow(dead_code)]
mod models;
mod parsers;
mod project_resolver;
mod session_sources;
mod startup;
#[allow(dead_code)]
mod ui;
mod utils;

use config::{APP_ID, GETTEXT_PACKAGE, LOCALEDIR, RESOURCES_FILE};
use gettextrs::{LocaleCategory, gettext};
use gtk::prelude::ApplicationExt;
use gtk::{gio, glib};
use relm4::{RelmApp, gtk, main_application};
use std::env;

use app::App;

use session_sources::{SessionSources, select_db_filename};
use tracing_subscriber::EnvFilter;

relm4::new_action_group!(AppActionGroup, "app");
relm4::new_stateless_action!(QuitAction, AppActionGroup, "quit");

fn main() {
    let invocation = startup::parse_invocation(env::args()).unwrap_or_else(|error| error.exit());
    let args = invocation.local;

    if args.print_db_path {
        let sources = SessionSources::resolve(args.sessions_dir.as_deref());
        let db_path = glib::user_data_dir()
            .join(APP_ID)
            .join(select_db_filename(sources.override_mode));
        println!("{}", db_path.display());
        return;
    }

    gtk::init().unwrap();
    sourceview5::init();
    relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);

    // Enable logging
    let log_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .with_env_filter(log_filter)
        .init();

    // setup gettext
    gettextrs::setlocale(LocaleCategory::LcAll, "");
    gettextrs::bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    gettextrs::textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    glib::set_application_name(&gettext("Sessions Chronicle"));

    let res = gio::Resource::load(RESOURCES_FILE).expect("Could not load gresource file");
    gio::resources_register(&res);

    gtk::Window::set_default_icon_name(APP_ID);

    let app = main_application();
    app.set_resource_base_path(Some("/dev/maciz/sessionschronicle/"));

    let app = RelmApp::from_app(app).with_args(invocation.gapplication_args);
    app.allow_multiple_instances(startup::allow_multiple_instances(
        args.sessions_dir.as_deref(),
    ));

    let data = res
        .lookup_data(
            "/dev/maciz/sessionschronicle/style.css",
            gio::ResourceLookupFlags::NONE,
        )
        .unwrap();
    relm4::set_global_css(&glib::GString::from_utf8_checked(data.to_vec()).unwrap());
    app.visible_on_activate(false).run::<App>(args.sessions_dir);
}
