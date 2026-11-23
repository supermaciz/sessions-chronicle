use chrono::{DateTime, Local};
use gtk::prelude::*;
use relm4::{RelmWidgetExt, WidgetTemplate, gtk};

use crate::database::SessionModels;
use crate::models::Session;
use crate::ui::activity_bar::SessionActivityBar;
use crate::ui::format::format_count;

#[relm4::widget_template(pub(super))]
impl WidgetTemplate for SessionSummary {
    view! {
        gtk::ScrolledWindow {
            set_hscrollbar_policy: gtk::PolicyType::Never,
            set_vscrollbar_policy: gtk::PolicyType::Automatic,
            set_propagate_natural_height: true,
            set_max_content_height: 520,

            #[wrap(Some)]
            set_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 12,
                set_margin_all: 16,

                #[name = "project_label"]
                gtk::Label {
                    add_css_class: "title-2",
                    set_halign: gtk::Align::Start,
                    set_wrap: true,
                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                },

                #[name = "path_label"]
                gtk::Label {
                    add_css_class: "dim-label",
                    set_halign: gtk::Align::Start,
                    set_wrap: true,
                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                    set_selectable: true,
                },

                #[name = "session_id_row"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 6,

                    gtk::Label {
                        set_label: "Session ID:",
                        add_css_class: "dim-label",
                    },

                    #[name = "session_id_label"]
                    gtk::Label {
                        add_css_class: "monospace",
                        set_selectable: true,
                        set_wrap: true,
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                    },
                },

                #[name = "chip_row"]
                gtk::FlowBox {
                    set_selection_mode: gtk::SelectionMode::None,
                    set_row_spacing: 8,
                    set_column_spacing: 8,
                    set_max_children_per_line: 4,
                    set_min_children_per_line: 1,

                    append = &gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 6,
                        add_css_class: "pill",

                        #[name = "tool_icon"]
                        gtk::Image {
                            set_pixel_size: 16,
                        },

                        #[name = "tool_label"]
                        gtk::Label {},
                    },

                    append = &gtk::Box {
                        add_css_class: "pill",

                        #[name = "duration_chip"]
                        gtk::Label {},
                    },

                    append = &gtk::Box {
                        add_css_class: "pill",

                        #[name = "message_count_chip"]
                        gtk::Label {},
                    },

                    append = &gtk::Box {
                        add_css_class: "pill",

                        #[name = "ending_status_chip"]
                        gtk::Label {},
                    },
                },

                #[name = "first_prompt_separator"]
                gtk::Separator {},

                #[name = "first_prompt_section"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 4,

                    gtk::Label {
                        set_label: "FIRST PROMPT",
                        add_css_class: "section-heading",
                        set_halign: gtk::Align::Start,
                    },

                    #[name = "first_prompt_label"]
                    gtk::Label {
                        set_halign: gtk::Align::Start,
                        set_xalign: 0.0,
                        set_wrap: true,
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_lines: 3,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        set_max_width_chars: 80,
                    },
                },

                #[name = "activity_separator"]
                gtk::Separator {},

                #[name = "activity_section"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 8,

                    gtk::Label {
                        set_label: "ACTIVITY",
                        add_css_class: "section-heading",
                        set_halign: gtk::Align::Start,
                    },

                    #[name = "activity_bar"]
                    SessionActivityBar {
                        add_css_class: "activity-bar",
                    },

                    #[name = "legend_row"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,

                        #[name = "edit_legend"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,

                            gtk::Box {
                                add_css_class: "activity-edits",
                                set_size_request: (8, 8),
                                set_valign: gtk::Align::Center,
                            },

                            #[name = "edit_count_label"]
                            gtk::Label {
                                add_css_class: "dim-label",
                            },
                        },

                        #[name = "command_legend"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,

                            gtk::Box {
                                add_css_class: "activity-commands",
                                set_size_request: (8, 8),
                                set_valign: gtk::Align::Center,
                            },

                            #[name = "command_count_label"]
                            gtk::Label {
                                add_css_class: "dim-label",
                            },
                        },

                        #[name = "read_legend"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,

                            gtk::Box {
                                add_css_class: "activity-reads",
                                set_size_request: (8, 8),
                                set_valign: gtk::Align::Center,
                            },

                            #[name = "read_count_label"]
                            gtk::Label {
                                add_css_class: "dim-label",
                            },
                        },
                    },

                    #[name = "conversation_only_label"]
                    gtk::Label {
                        set_label: "Conversation only",
                        add_css_class: "dim-label",
                        set_halign: gtk::Align::Start,
                    },
                },

                #[name = "models_separator"]
                gtk::Separator {
                    set_visible: false,
                },

                #[name = "models_section"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 8,
                    set_visible: false,

                    gtk::Label {
                        set_label: "MODELS",
                        add_css_class: "section-heading",
                        set_halign: gtk::Align::Start,
                    },

                    #[name = "models_rows"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 8,
                    },

                    #[name = "models_footer"]
                    gtk::Label {
                        add_css_class: "caption",
                        add_css_class: "dim-label",
                        set_halign: gtk::Align::Start,
                        set_visible: false,
                    },
                },

                #[name = "tokens_separator"]
                gtk::Separator {},

                #[name = "tokens_section"]
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 8,

                    gtk::Label {
                        set_label: "TOKENS",
                        add_css_class: "section-heading",
                        set_halign: gtk::Align::Start,
                    },

                    #[name = "tokens_grid"]
                    gtk::FlowBox {
                        set_selection_mode: gtk::SelectionMode::None,
                        set_row_spacing: 8,
                        set_column_spacing: 16,
                        set_homogeneous: true,
                        set_max_children_per_line: 4,
                        set_min_children_per_line: 2,

                        #[name = "input_pair"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,

                            #[name = "input_value_label"]
                            gtk::Label {
                                add_css_class: "token-value",
                                set_halign: gtk::Align::Start,
                            },

                            gtk::Label {
                                set_label: "Input",
                                add_css_class: "dim-label",
                                set_halign: gtk::Align::Start,
                            },
                        },

                        #[name = "output_pair"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,

                            #[name = "output_value_label"]
                            gtk::Label {
                                add_css_class: "token-value",
                                set_halign: gtk::Align::Start,
                            },

                            gtk::Label {
                                set_label: "Output",
                                add_css_class: "dim-label",
                                set_halign: gtk::Align::Start,
                            },
                        },

                        #[name = "cache_pair"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,

                            #[name = "cache_value_label"]
                            gtk::Label {
                                add_css_class: "token-value",
                                set_halign: gtk::Align::Start,
                            },

                            gtk::Label {
                                set_label: "Cache",
                                add_css_class: "dim-label",
                                set_halign: gtk::Align::Start,
                            },
                        },

                        #[name = "reasoning_pair"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 2,

                            #[name = "reasoning_value_label"]
                            gtk::Label {
                                add_css_class: "token-value",
                                set_halign: gtk::Align::Start,
                            },

                            gtk::Label {
                                set_label: "Reasoning",
                                add_css_class: "dim-label",
                                set_halign: gtk::Align::Start,
                            },
                        },
                    },
                },
            },
        }
    }
}
impl SessionSummary {
    pub(super) fn widget(&self) -> &gtk::ScrolledWindow {
        self.as_ref()
    }

    pub(super) fn update(&self, session: &Session) {
        self.update_session_header(session);
        self.update_chip_row(session);
        self.update_first_prompt(session);
        self.update_activity_section(session);
        self.update_tokens_section(session);
    }

    pub(super) fn update_models(
        &self,
        models: &SessionModels,
        jump: impl Fn(i64) + Clone + 'static,
    ) {
        while let Some(row) = self.models_rows.first_child() {
            self.models_rows.remove(&row);
        }

        let has_models = !models.models.is_empty();
        let show_footer = has_models && models.unattributed_count > 0;
        self.models_footer.set_visible(show_footer);
        if show_footer {
            self.models_footer.set_label(&format!(
                "{} entries with no recorded model",
                models.unattributed_count
            ));
        }

        for usage in &models.models {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let slug = gtk::Label::builder()
                .label(&usage.model)
                .css_classes(["monospace"])
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .tooltip_text(&usage.model)
                .hexpand(true)
                .xalign(0.0)
                .build();
            row.append(&slug);

            let count = gtk::Label::new(Some(&format_count(usage.entry_count, "entry", "entries")));
            count.add_css_class("dim-label");
            row.append(&count);

            if let Some(timestamp) = usage
                .first_used_at
                .and_then(|time| DateTime::from_timestamp(time, 0))
            {
                let local_time = timestamp.with_timezone(&Local);
                let time =
                    gtk::Label::new(Some(&format!("first used {}", local_time.format("%H:%M"))));
                time.add_css_class("dim-label");
                row.append(&time);
            }

            if models.models.len() > 1 {
                let button = gtk::Button::builder()
                    .icon_name("go-down-symbolic")
                    .css_classes(["flat", "circular"])
                    .tooltip_text("Jump to first use")
                    .build();
                button.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Jump to first use of {}",
                    usage.model
                ))]);
                let index = usage.first_item_index;
                let jump = jump.clone();
                button.connect_clicked(move |_| jump(index));
                row.append(&button);
            }

            self.models_rows.append(&row);
        }

        self.models_separator.set_visible(has_models);
        self.models_section.set_visible(has_models);
    }

    fn update_session_header(&self, session: &Session) {
        let project_name = session
            .project_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown project");
        self.project_label.set_label(project_name);

        let path = session
            .project_path
            .as_deref()
            .unwrap_or(&session.file_path);
        self.path_label.set_label(path);

        self.tool_icon.set_icon_name(Some(session.tool.icon_name()));
        self.tool_label.set_label(session.tool.display_name());

        self.session_id_label.set_label(&session.id);
    }

    fn update_chip_row(&self, session: &Session) {
        self.duration_chip
            .set_label(&crate::ui::format::format_session_duration(
                session.start_time,
                session.last_updated,
            ));
        self.message_count_chip
            .set_label(&crate::ui::format::format_count(
                session.message_count,
                "message",
                "messages",
            ));
        self.ending_status_chip
            .set_label(crate::ui::format::format_ending_label(
                &session.ending_status,
            ));
        self.ending_status_chip.set_css_classes(&[
            "pill",
            crate::ui::format::ending_css_class(&session.ending_status),
        ]);
        self.ending_status_chip
            .update_property(&[gtk::accessible::Property::Label(
                crate::ui::format::format_ending_accessible_label(&session.ending_status),
            )]);
    }

    fn update_first_prompt(&self, session: &Session) {
        let has_first_prompt = session
            .first_prompt
            .as_ref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false);
        self.first_prompt_section.set_visible(has_first_prompt);
        self.first_prompt_separator.set_visible(has_first_prompt);
        if has_first_prompt {
            self.first_prompt_label
                .set_label(session.first_prompt.as_ref().unwrap());
        }
    }

    fn update_activity_section(&self, session: &Session) {
        let has_activity =
            session.edit_count > 0 || session.command_count > 0 || session.read_count > 0;
        self.activity_section.set_visible(true);
        self.activity_bar.set_visible(has_activity);
        self.legend_row.set_visible(has_activity);
        self.conversation_only_label.set_visible(!has_activity);

        if has_activity {
            self.activity_bar.set_counts(
                session.edit_count,
                session.command_count,
                session.read_count,
            );

            self.edit_count_label
                .set_label(&crate::ui::format::format_count(
                    session.edit_count,
                    "edit",
                    "edits",
                ));
            self.command_count_label
                .set_label(&crate::ui::format::format_count(
                    session.command_count,
                    "command",
                    "commands",
                ));
            self.read_count_label
                .set_label(&crate::ui::format::format_count(
                    session.read_count,
                    "read",
                    "reads",
                ));

            self.edit_legend.set_visible(session.edit_count > 0);
            self.command_legend.set_visible(session.command_count > 0);
            self.read_legend.set_visible(session.read_count > 0);
        } else {
            self.activity_bar.set_counts(0, 0, 0);
        }
    }

    fn update_tokens_section(&self, session: &Session) {
        let has_tokens = session.token_usage.is_some();
        self.tokens_section.set_visible(has_tokens);
        self.tokens_separator.set_visible(has_tokens);

        if let Some(usage) = &session.token_usage {
            self.input_value_label
                .set_label(&crate::ui::format::format_token_count(usage.input_tokens));
            self.output_value_label
                .set_label(&crate::ui::format::format_token_count(usage.output_tokens));

            let has_cache = usage.cache_read_tokens.is_some() || usage.cache_write_tokens.is_some();
            self.cache_pair.set_visible(has_cache);
            if has_cache && let Some(cache_text) = crate::ui::format::format_token_cache(usage) {
                self.cache_value_label.set_label(&cache_text);
            }

            let has_reasoning = usage.reasoning_tokens.is_some();
            self.reasoning_pair.set_visible(has_reasoning);
            if let Some(reasoning) = usage.reasoning_tokens {
                self.reasoning_value_label
                    .set_label(&crate::ui::format::format_token_count(reasoning));
            }

            self.tokens_section
                .set_tooltip_text(Some(crate::ui::format::token_semantics_help_tooltip()));

            self.input_pair
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "Input tokens: {}",
                    crate::ui::format::format_token_count(usage.input_tokens)
                ))]);
            self.output_pair
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "Output tokens: {}",
                    crate::ui::format::format_token_count(usage.output_tokens)
                ))]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{SessionModelUsage, SessionModels};
    use chrono::{TimeZone, Utc};
    use std::{cell::Cell, rc::Rc};

    #[gtk::test]
    fn model_summary_shows_only_recorded_models_and_links_multi_model_rows() {
        let summary = SessionSummary::init(());
        let empty_section = &summary.models_section;
        assert!(!empty_section.is_visible());
        summary.update_models(
            &SessionModels {
                models: vec![],
                unattributed_count: 2,
            },
            |_| unreachable!(),
        );
        assert!(!empty_section.is_visible());
        assert!(!summary.models_separator.is_visible());

        let opus = SessionModelUsage {
            model: "opus".into(),
            entry_count: 1,
            first_item_index: 0,
            first_used_at: None,
        };
        summary.update_models(
            &SessionModels {
                models: vec![opus.clone()],
                unattributed_count: 0,
            },
            |_| unreachable!(),
        );
        assert!(summary.models_section.is_visible());
        assert!(summary.models_separator.is_visible());
        assert!(!summary.models_footer.is_visible());
        let single_row = summary.models_rows.first_child().unwrap();
        let single_row_children = single_row.observe_children();
        let single_row_texts: Vec<_> = single_row_children
            .iter::<gtk::glib::Object>()
            .filter_map(|child| child.unwrap().downcast::<gtk::Label>().ok())
            .map(|label| label.text().to_string())
            .collect();
        let single_button_count = single_row_children
            .iter::<gtk::glib::Object>()
            .filter(|child| child.as_ref().unwrap().is::<gtk::Button>())
            .count();
        assert_eq!(single_row_texts, ["opus", "1 entry"]);
        assert_eq!(single_button_count, 0);

        let activated_index = Rc::new(Cell::new(None));
        let sonnet = "sonnet<&-literal";
        let timestamp = 1_700_000_000;
        summary.update_models(
            &SessionModels {
                models: vec![
                    opus,
                    SessionModelUsage {
                        model: sonnet.into(),
                        entry_count: 3,
                        first_item_index: 8,
                        first_used_at: Some(timestamp),
                    },
                ],
                unattributed_count: 2,
            },
            {
                let activated_index = activated_index.clone();
                move |index| activated_index.set(Some(index))
            },
        );

        let footer = &summary.models_footer;
        assert!(footer.is_visible());
        assert!(footer.has_css_class("caption"));
        assert!(footer.has_css_class("dim-label"));
        assert_eq!(footer.text(), "2 entries with no recorded model");
        assert_eq!(summary.models_rows.observe_children().n_items(), 2);
        let first_row = summary.models_rows.first_child().unwrap();
        assert_eq!(
            first_row
                .first_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "opus"
        );
        assert!(first_row.last_child().unwrap().is::<gtk::Button>());
        let second_row = first_row.next_sibling().unwrap();
        let slug = second_row
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert_eq!(slug.text(), sonnet);
        assert!(slug.has_css_class("monospace"));
        assert_eq!(slug.ellipsize(), gtk::pango::EllipsizeMode::End);
        assert_eq!(slug.tooltip_text().as_deref(), Some(sonnet));
        let count = slug
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert_eq!(count.text(), "3 entries");
        assert!(count.has_css_class("dim-label"));
        let time = count
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let local = chrono::Local.timestamp_opt(timestamp, 0).unwrap();
        assert_eq!(time.text(), format!("first used {}", local.format("%H:%M")));
        assert!(time.has_css_class("dim-label"));
        let button = second_row
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(button.icon_name().as_deref(), Some("go-down-symbolic"));
        assert_eq!(button.tooltip_text().as_deref(), Some("Jump to first use"));
        assert!(button.has_css_class("flat"));
        assert!(button.has_css_class("circular"));

        // gtk-rs does not expose the value-checking varargs test API safely.
        use gtk::glib::translate::{ToGlibPtr, from_glib_full};
        let accessible: &gtk::Accessible = button.upcast_ref();
        // SAFETY: Label takes one NUL-terminated string, and the widget is alive.
        let mismatch = unsafe {
            let result = gtk::ffi::gtk_test_accessible_check_property(
                accessible.to_glib_none().0,
                gtk::ffi::GTK_ACCESSIBLE_PROPERTY_LABEL,
                c"Jump to first use of sonnet<&-literal".as_ptr(),
            );
            (!result.is_null()).then(|| from_glib_full::<_, gtk::glib::GString>(result))
        };
        assert_eq!(mismatch, None, "incorrect accessible jump label");
        button.emit_clicked();
        assert_eq!(activated_index.get(), Some(8));
    }

    fn build_test_session(
        first_prompt: Option<&str>,
        token_usage: Option<crate::models::TokenUsage>,
        edit_count: usize,
        command_count: usize,
        read_count: usize,
    ) -> Session {
        Session {
            id: "test-session-123".to_string(),
            tool: crate::models::AiAssistant::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: Utc.with_ymd_and_hms(2026, 3, 30, 10, 0, 0).unwrap(),
            message_count: 42,
            file_path: "/tmp/test.json".to_string(),
            last_updated: Utc.with_ymd_and_hms(2026, 3, 30, 12, 14, 0).unwrap(),
            pinned_at: None,
            first_prompt: first_prompt.map(str::to_string),
            parent_session_id: None,
            is_subagent: false,
            token_usage,
            edit_count,
            read_count,
            command_count,
            ending_status: crate::models::SessionEndingStatus::Clean,
        }
    }

    #[gtk::test]
    fn hides_optional_sections_when_data_is_missing() {
        let summary = SessionSummary::init(());

        summary.update(&build_test_session(None, None, 0, 0, 0));

        assert!(!summary.first_prompt_section.is_visible());
        assert!(!summary.tokens_section.is_visible());
        assert!(summary.activity_bar.has_css_class("activity-bar"));
    }

    #[gtk::test]
    fn populates_identity_prompt_and_tokens() {
        let summary = SessionSummary::init(());

        summary.update(&build_test_session(
            Some("Ship the summary header"),
            Some(crate::models::TokenUsage {
                input_tokens: 1200,
                output_tokens: 300,
                cache_read_tokens: Some(400),
                cache_write_tokens: None,
                reasoning_tokens: Some(50),
            }),
            4,
            2,
            1,
        ));

        assert_eq!(summary.project_label.label(), "project");
        assert_eq!(summary.duration_chip.label(), "2h 14m");
        assert_eq!(summary.message_count_chip.label(), "42 messages");
        assert_eq!(summary.ending_status_chip.label(), "Ended cleanly");
        assert_eq!(
            summary.first_prompt_label.label(),
            "Ship the summary header"
        );
        assert!(summary.input_value_label.label().starts_with('1'));
        assert!(summary.input_value_label.label().ends_with("200"));
        assert_eq!(summary.output_value_label.label(), "300");
    }

    #[gtk::test]
    fn uses_conversation_only_when_activity_counts_are_zero() {
        let summary = SessionSummary::init(());

        summary.update(&build_test_session(Some("Only chat"), None, 0, 0, 0));

        assert_eq!(summary.conversation_only_label.label(), "Conversation only");
        assert!(!summary.activity_bar.is_visible());
    }

    #[gtk::test]
    fn activity_bar_does_not_lock_a_minimum_width_request() {
        let summary = SessionSummary::init(());

        summary.update(&build_test_session(Some("Ship it"), None, 4, 2, 1));

        assert_eq!(summary.activity_bar.width_request(), -1);
    }

    #[gtk::test]
    fn applies_status_and_activity_css_classes() {
        let summary = SessionSummary::init(());
        let mut session = build_test_session(None, None, 0, 0, 0);
        session.ending_status = crate::models::SessionEndingStatus::Error;

        summary.update(&session);

        assert!(summary.ending_status_chip.has_css_class("pill"));
        assert!(summary.ending_status_chip.has_css_class("ending-failed"));
        assert!(summary.activity_bar.has_css_class("activity-bar"));
    }
}
