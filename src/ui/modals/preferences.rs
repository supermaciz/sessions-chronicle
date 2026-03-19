use std::path::PathBuf;

use adw::prelude::{
    ActionRowExt, AdwDialogExt, AlertDialogExt, ComboRowExt, PreferencesDialogExt,
    PreferencesGroupExt, PreferencesPageExt,
};
use gtk::gio;
use gtk::glib;
use gtk::prelude::{ButtonExt, DisplayExt, SettingsExt, WidgetExt};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

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

pub struct PreferencesDialog {
    root: adw::PreferencesDialog,
}

#[derive(Debug)]
pub enum PreferencesInput {
    ResetClicked,
    ResetConfirmed,
}

#[derive(Debug)]
pub enum PreferencesOutput {
    ReindexRequested,
}

impl SimpleComponent for PreferencesDialog {
    type Init = PathBuf;
    type Widgets = ();
    type Input = PreferencesInput;
    type Output = PreferencesOutput;
    type Root = adw::PreferencesDialog;

    fn init_root() -> Self::Root {
        adw::PreferencesDialog::builder().build()
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
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

        // Session Resumption group
        let resumption_group = adw::PreferencesGroup::builder()
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

        resumption_group.add(&combo_row);
        page.add(&resumption_group);

        // Advanced group
        let advanced_group = adw::PreferencesGroup::builder().title("Advanced").build();

        // Database location row with copy button
        let db_path_str = db_path.to_string_lossy().to_string();
        let db_row = adw::ActionRow::builder()
            .title("Database Location")
            .subtitle(format!(
                "<tt>{}</tt>",
                glib::markup_escape_text(&db_path_str)
            ))
            .subtitle_lines(1)
            .activatable(false)
            .build();

        let copy_button = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Copy path to clipboard")
            .css_classes(["flat"])
            .build();

        {
            let db_path_str = db_path_str.clone();
            let root = root.clone();
            copy_button.connect_clicked(move |button| {
                let clipboard = button.display().clipboard();
                clipboard.set_text(&db_path_str);
                let toast = adw::Toast::new("Path copied to clipboard");
                toast.set_timeout(2);
                root.add_toast(toast);
            });
        }

        db_row.add_suffix(&copy_button);
        advanced_group.add(&db_row);

        let reset_row = adw::ActionRow::builder()
            .title("Reset session index")
            .subtitle("Clear and rebuild the entire session index from source files")
            .build();

        let reset_button = gtk::Button::builder()
            .label("Reset")
            .valign(gtk::Align::Center)
            .css_classes(["destructive-action"])
            .build();

        let input_sender = sender.input_sender().clone();
        reset_button.connect_clicked(move |_| {
            input_sender.send(PreferencesInput::ResetClicked).ok();
        });

        reset_row.add_suffix(&reset_button);
        advanced_group.add(&reset_row);
        page.add(&advanced_group);

        root.add(&page);

        // Do NOT present here — the parent controls visibility.

        let model = Self { root: root.clone() };
        let widgets = ();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            PreferencesInput::ResetClicked => {
                let dialog = adw::AlertDialog::builder()
                    .heading("Reset session index?")
                    .body("This will clear and rebuild the entire session index from source files.")
                    .build();
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("confirm", "Reset");
                dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");

                let input_sender = sender.input_sender().clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "confirm" {
                        input_sender.send(PreferencesInput::ResetConfirmed).ok();
                    }
                });

                dialog.present(Some(&self.root));
            }
            PreferencesInput::ResetConfirmed => {
                sender.output(PreferencesOutput::ReindexRequested).unwrap();
            }
        }
    }

    fn update_view(&self, _widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {}
}
