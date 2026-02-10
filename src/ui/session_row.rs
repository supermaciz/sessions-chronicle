use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::glib::prelude::ObjectExt;
use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::{adw, gtk};

use adw::prelude::ActionRowExt;

use crate::models::{Session, Tool};

/// Data passed to initialize each factory row.
pub struct SessionRowInit {
    pub session: Session,
}

/// A single session row inside the ListBox, managed by FactoryVecDeque.
#[derive(Debug)]
pub struct SessionRow {
    session: Session,
}

const SESSION_ID_KEY: &str = "session-id";

#[derive(Debug)]
pub enum SessionRowOutput {
    ResumeRequested(String, Tool),
}

#[relm4::factory(pub)]
impl FactoryComponent for SessionRow {
    type Init = SessionRowInit;
    type Input = ();
    type Output = SessionRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        root = gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,

            append = &adw::ActionRow::builder()
                .title(Self::session_title(&self.session))
                .subtitle(Self::session_subtitle(&self.session))
                .activatable(true)
                .build() {
                set_hexpand: true,

                add_prefix = &gtk::Image::from_icon_name(self.session.tool.icon_name()) {
                    set_pixel_size: 16,
                },

                add_suffix = &gtk::Button::from_icon_name("utilities-terminal-symbolic") {
                    add_css_class: "flat",
                    set_tooltip_text: Some("Resume in terminal"),
                    connect_clicked[sender, session_id = self.session.id.clone(), tool = self.session.tool] => move |_| {
                        let _ = sender.output(SessionRowOutput::ResumeRequested(session_id.clone(), tool));
                    },
                },

                add_suffix = &gtk::Image::from_icon_name("go-next-symbolic") {
                    add_css_class: "dim-label",
                },

                add_suffix = &gtk::Label {
                    add_css_class: "dim-label",
                    set_halign: gtk::Align::End,
                    set_label: &Self::format_relative_time(self.session.last_updated),
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            session: init.session,
        }
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        root: Self::Root,
        returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let session_id = self.session.id.clone();
        unsafe {
            returned_widget.set_data(SESSION_ID_KEY, session_id.clone());
        }

        let widgets = view_output!();
        widgets
    }
}

impl SessionRow {
    fn session_title(session: &Session) -> String {
        session
            .project_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| "Unknown project".to_string())
    }

    fn session_subtitle(session: &Session) -> String {
        let detail = session
            .project_path
            .as_deref()
            .unwrap_or(&session.file_path);
        format!("{} • {} messages", detail, session.message_count)
    }

    fn format_relative_time(instant: DateTime<Utc>) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(instant);

        if duration < ChronoDuration::minutes(1) {
            "Just now".to_string()
        } else if duration < ChronoDuration::hours(1) {
            format!("{}m ago", duration.num_minutes())
        } else if duration < ChronoDuration::days(1) {
            format!("{}h ago", duration.num_hours())
        } else if duration < ChronoDuration::days(7) {
            format!("{}d ago", duration.num_days())
        } else {
            instant.format("%Y-%m-%d").to_string()
        }
    }
}
