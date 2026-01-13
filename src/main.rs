#[rustfmt::skip]
mod config;
mod app;
mod database;
mod models;
mod parsers;
mod ui;

use config::{APP_ID, GETTEXT_PACKAGE, LOCALEDIR, RESOURCES_FILE};
use gettextrs::{LocaleCategory, gettext};
use gtk::prelude::ApplicationExt;
use gtk::{gio, glib};
use relm4::{RelmApp, gtk, main_application};
use std::{env, path::PathBuf};

use app::App;

use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, value_name = "DIR")]
    sessions_dir: Option<PathBuf>,

    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    gtk_options: Vec<String>,
}

relm4::new_action_group!(AppActionGroup, "app");
relm4::new_stateless_action!(QuitAction, AppActionGroup, "quit");

fn main() {
    let args = Args::parse();

    gtk::init().unwrap();

    // Enable logging
    tracing_subscriber::fmt()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .with_max_level(tracing::Level::INFO)
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
    app.set_resource_base_path(Some("/io/github/supermaciz/sessionschronicle/"));

    let program_invocation = env::args()
        .next()
        .unwrap_or_else(|| String::from("sessions-chronicle"));
    let mut gtk_args = vec![program_invocation];
    gtk_args.extend(args.gtk_options.clone());

    let app = RelmApp::from_app(app).with_args(gtk_args);

    let data = res
        .lookup_data(
            "/io/github/supermaciz/sessionschronicle/style.css",
            gio::ResourceLookupFlags::NONE,
        )
        .unwrap();
    relm4::set_global_css(&glib::GString::from_utf8_checked(data.to_vec()).unwrap());
    app.visible_on_activate(false).run::<App>(args.sessions_dir);
}
