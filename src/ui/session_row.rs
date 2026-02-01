use chrono::{DateTime, Duration as ChronoDuration, Utc};
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

#[derive(Debug)]
pub enum SessionRowOutput {
    Selected(String),
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
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        // Build an ActionRow manually since it needs complex suffix widgets.
        // We replace the generated root box with our ActionRow content.
        let row = adw::ActionRow::builder()
            .title(Self::session_title(&self.session))
            .subtitle(Self::session_subtitle(&self.session))
            .activatable(true)
            .build();

        let icon = gtk::Image::from_icon_name(self.session.tool.icon_name());
        icon.set_pixel_size(16);
        row.add_prefix(&icon);

        // Resume button
        let resume_button = gtk::Button::from_icon_name("utilities-terminal-symbolic");
        resume_button.add_css_class("flat");
        resume_button.set_tooltip_text(Some("Resume in terminal"));
        let session_id = self.session.id.clone();
        let tool = self.session.tool;
        let sender_resume = sender.clone();
        resume_button.connect_clicked(move |_| {
            let _ =
                sender_resume.output(SessionRowOutput::ResumeRequested(session_id.clone(), tool));
        });
        row.add_suffix(&resume_button);

        // Chevron
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        row.add_suffix(&chevron);

        // Time label
        let time_label =
            gtk::Label::new(Some(&Self::format_relative_time(self.session.last_updated)));
        time_label.add_css_class("dim-label");
        time_label.set_halign(gtk::Align::End);
        row.add_suffix(&time_label);

        // Clicking the row emits Selected
        let session_id_click = self.session.id.clone();
        let sender_click = sender.clone();
        row.connect_activated(move |_| {
            let _ = sender_click.output(SessionRowOutput::Selected(session_id_click.clone()));
        });

        // Append ActionRow inside the root box so the ListBox picks it up.
        root.append(&row);

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
