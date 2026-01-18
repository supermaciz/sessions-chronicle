use adw::prelude::{
    AdwDialogExt, ComboRowExt, PreferencesDialogExt, PreferencesGroupExt, PreferencesPageExt,
};
use gtk::gio;
use gtk::prelude::GtkApplicationExt;
use gtk::prelude::SettingsExt;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk, main_application};

use crate::config::APP_ID;
use crate::utils::terminal::Terminal;

pub struct PreferencesDialog {}

#[derive(Debug)]
pub enum PreferencesMsg {}

impl SimpleComponent for PreferencesDialog {
    type Init = ();
    type Widgets = PreferencesWidgets;
    type Input = PreferencesMsg;
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

        let page = adw::PreferencesPage::builder().title("General").build();

        let group = adw::PreferencesGroup::builder()
            .title("Session Resumption")
            .build();

        let combo_model = gio::ListStore::new::<gtk::StringObject>();
        let terminals: Vec<Terminal> = vec![
            Terminal::Auto,
            Terminal::Ptyxis,
            Terminal::Ghostty,
            Terminal::Foot,
            Terminal::Alacritty,
            Terminal::Kitty,
        ];

        let mut selected_index = 0u32;
        for (i, terminal) in terminals.iter().enumerate() {
            let string_obj = gtk::StringObject::new(terminal.display_name());
            combo_model.append(&string_obj);
            if current_terminal.as_str() == terminal.to_str() {
                selected_index = i as u32;
            }
        }

        let combo_row = adw::ComboRow::builder()
            .title("Terminal")
            .subtitle("Terminal emulator for resuming sessions")
            .model(&combo_model)
            .selected(selected_index)
            .build();

        let settings_clone = settings.clone();
        combo_row.connect_selected_notify(move |row: &adw::ComboRow| {
            let selected = row.selected();
            if let Some(terminal) = terminals.get(selected as usize) {
                let _ = settings_clone.set_string("resume-terminal", terminal.to_str());
            }
        });

        group.add(&combo_row);
        page.add(&group);
        root.add(&page);

        let model = Self {};
        let widgets = PreferencesWidgets { root };

        widgets.root.present(Some(&main_application().windows()[0]));

        ComponentParts { model, widgets }
    }

    fn update_view(&self, _widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {}
}

pub struct PreferencesWidgets {
    root: adw::PreferencesDialog,
}
