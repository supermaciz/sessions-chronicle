use adw::prelude::{
    AdwDialogExt, ComboRowExt, PreferencesDialogExt, PreferencesGroupExt, PreferencesPageExt,
};
use gtk::gio;
use gtk::prelude::{GtkApplicationExt, SettingsExt};
use relm4::{adw, gtk, main_application, ComponentParts, ComponentSender, SimpleComponent};

use crate::config::APP_ID;
use crate::utils::terminal::Terminal;

const TERMINALS: &[Terminal] = &[
    Terminal::Auto,
    Terminal::Ptyxis,
    Terminal::Ghostty,
    Terminal::Foot,
    Terminal::Alacritty,
    Terminal::Kitty,
];

pub struct PreferencesDialog;

impl SimpleComponent for PreferencesDialog {
    type Init = ();
    type Widgets = ();
    type Input = ();
    type Output = ();
    type Root = adw::PreferencesDialog;

    fn init_root() -> Self::Root {
        adw::PreferencesDialog::builder().build()
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = gio::Settings::new(APP_ID);
        let current_terminal = settings.string("resume-terminal");

        let combo_model = gio::ListStore::new::<gtk::StringObject>();
        let mut selected_index = 0u32;
        for (i, terminal) in TERMINALS.iter().enumerate() {
            combo_model.append(&gtk::StringObject::new(terminal.display_name()));
            if current_terminal.as_str() == terminal.to_str() {
                selected_index = i as u32;
            }
        }

        let page = adw::PreferencesPage::builder().title("General").build();

        let group = adw::PreferencesGroup::builder()
            .title("Session Resumption")
            .build();

        let combo_row = adw::ComboRow::builder()
            .title("Terminal")
            .subtitle("Terminal emulator for resuming sessions")
            .model(&combo_model)
            .selected(selected_index)
            .build();

        combo_row.connect_selected_notify(move |row| {
            let selected = row.selected();
            if let Some(terminal) = TERMINALS.get(selected as usize) {
                let _ = settings.set_string("resume-terminal", terminal.to_str());
            }
        });

        group.add(&combo_row);
        page.add(&group);
        root.add(&page);

        let model = Self;
        let widgets = ();

        root.present(Some(&main_application().windows()[0]));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, _message: Self::Input, _sender: ComponentSender<Self>) {}
    fn update_view(&self, _widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {}
}
