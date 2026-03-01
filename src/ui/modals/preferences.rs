use adw::prelude::{
    ActionRowExt, AdwDialogExt, AlertDialogExt, ComboRowExt, PreferencesDialogExt,
    PreferencesGroupExt, PreferencesPageExt,
};
use gtk::gio;
use gtk::prelude::{ButtonExt, EditableExt, SettingsExt};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use crate::config::APP_ID;
use crate::utils::terminal::Terminal;
use crate::utils::title_generator::{TitleProvider, detect_available_provider};

const TERMINALS: &[Terminal] = &[
    Terminal::Auto,
    Terminal::Ptyxis,
    Terminal::Ghostty,
    Terminal::Foot,
    Terminal::Alacritty,
    Terminal::Kitty,
];

const TITLE_PROVIDER_LABELS: &[&str] = &["Auto", "OpenCode", "Claude"];

fn provider_value_to_index(value: &str) -> u32 {
    match value {
        "opencode" => 1,
        "claude" => 2,
        _ => 0,
    }
}

fn provider_index_to_value(index: u32) -> &'static str {
    match index {
        1 => "opencode",
        2 => "claude",
        _ => "auto",
    }
}

fn auto_provider_subtitle() -> &'static str {
    match detect_available_provider() {
        Some(TitleProvider::OpenCode) => "Auto (OpenCode detected)",
        Some(TitleProvider::Claude) => "Auto (Claude detected)",
        _ => "Auto (No CLI detected)",
    }
}

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
    type Init = ();
    type Widgets = ();
    type Input = PreferencesInput;
    type Output = PreferencesOutput;
    type Root = adw::PreferencesDialog;

    fn init_root() -> Self::Root {
        adw::PreferencesDialog::builder().build()
    }

    fn init(
        _: Self::Init,
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

        let settings_for_terminal = settings.clone();
        combo_row.connect_selected_notify(move |row| {
            let selected = row.selected();
            if let Some(terminal) = TERMINALS.get(selected as usize) {
                let _ = settings_for_terminal.set_string("resume-terminal", terminal.to_str());
            }
        });

        resumption_group.add(&combo_row);
        page.add(&resumption_group);

        let ai_group = adw::PreferencesGroup::builder()
            .title("AI Session Titles")
            .build();

        let ai_enabled_row = adw::SwitchRow::builder()
            .title("Enable title generation")
            .subtitle("Generate titles for sessions missing native titles")
            .active(settings.boolean("ai-title-generation-enabled"))
            .build();
        let settings_for_enabled = settings.clone();
        ai_enabled_row.connect_active_notify(move |row| {
            let _ =
                settings_for_enabled.set_boolean("ai-title-generation-enabled", row.is_active());
        });

        let provider_model = gio::ListStore::new::<gtk::StringObject>();
        for label in TITLE_PROVIDER_LABELS {
            provider_model.append(&gtk::StringObject::new(label));
        }

        let selected_provider =
            provider_value_to_index(settings.string("ai-title-generation-provider").as_str());

        let provider_row = adw::ComboRow::builder()
            .title("Provider")
            .subtitle(auto_provider_subtitle())
            .model(&provider_model)
            .selected(selected_provider)
            .build();
        let settings_for_provider = settings.clone();
        provider_row.connect_selected_notify(move |row| {
            let value = provider_index_to_value(row.selected());
            let _ = settings_for_provider.set_string("ai-title-generation-provider", value);
        });

        let current_model = settings.string("ai-title-generation-model");
        let model_row = adw::EntryRow::builder().title("Model override").build();
        model_row.set_text(current_model.as_str());
        let settings_for_model = settings.clone();
        model_row.connect_changed(move |row| {
            let _ = settings_for_model.set_string("ai-title-generation-model", row.text().as_str());
        });

        ai_group.add(&ai_enabled_row);
        ai_group.add(&provider_row);
        ai_group.add(&model_row);
        page.add(&ai_group);

        // Advanced group with reset button
        let advanced_group = adw::PreferencesGroup::builder().title("Advanced").build();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_index_mapping_round_trips() {
        assert_eq!(provider_value_to_index("auto"), 0);
        assert_eq!(provider_value_to_index("opencode"), 1);
        assert_eq!(provider_value_to_index("claude"), 2);
        assert_eq!(provider_value_to_index("invalid"), 0);

        assert_eq!(provider_index_to_value(0), "auto");
        assert_eq!(provider_index_to_value(1), "opencode");
        assert_eq!(provider_index_to_value(2), "claude");
        assert_eq!(provider_index_to_value(99), "auto");
    }
}
