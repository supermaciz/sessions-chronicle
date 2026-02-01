use gtk::glib::prelude::ObjectExt;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{adw, gtk, ComponentParts, ComponentSender, SimpleComponent};
use std::path::{Path, PathBuf};

use crate::database::{load_sessions, search_sessions};
use crate::models::{Session, Tool};
use crate::ui::session_row::{SessionRow, SessionRowInit, SessionRowOutput};

#[derive(Debug)]
pub struct SessionList {
    db_path: PathBuf,
    active_tools: Vec<Tool>,
    search_query: String,
    all_tools_selected: bool,
    sessions: FactoryVecDeque<SessionRow>,
}

#[derive(Debug)]
pub enum SessionListMsg {
    SetTools(Vec<Tool>),
    SetSearchQuery(String),
    SessionSelected(String),
    ResumeRequested(String, Tool),
}

#[derive(Debug)]
pub enum SessionListOutput {
    SessionSelected(String),
    ResumeRequested(String, Tool),
}

#[relm4::component(pub)]
impl SimpleComponent for SessionList {
    type Init = PathBuf;
    type Input = SessionListMsg;
    type Output = SessionListOutput;
    type Widgets = SessionListWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,
            set_vexpand: true,

            #[name = "content_stack"]
            gtk::Stack {
                set_vexpand: true,
                set_hexpand: true,

                #[name = "empty_state"]
                adw::StatusPage {
                    set_vexpand: true,
                    set_icon_name: Some("document-open-recent-symbolic"),
                    set_title: "No Sessions Yet",
                    set_description: Some("Your AI coding sessions will appear here"),
                },

                #[name = "session_list_scroller"]
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[local_ref]
                    session_list_box -> gtk::ListBox {
                        add_css_class: "boxed-list",
                        set_selection_mode: gtk::SelectionMode::None,
                    }
                }
            }
        }
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let active_tools = vec![Tool::ClaudeCode, Tool::OpenCode, Tool::Codex];
        let search_query = String::new();
        let fetched = Self::fetch_sessions(&db_path, &active_tools, &search_query);

        let sessions: FactoryVecDeque<SessionRow> = FactoryVecDeque::builder()
            .launch_default()
            .forward(sender.input_sender(), |msg| match msg {
                SessionRowOutput::ResumeRequested(id, tool) => {
                    SessionListMsg::ResumeRequested(id, tool)
                }
            });

        let mut model = Self {
            db_path,
            active_tools,
            search_query,
            all_tools_selected: true,
            sessions,
        };

        // Populate initial data
        {
            let mut guard = model.sessions.guard();
            for session in fetched {
                guard.push_back(SessionRowInit { session });
            }
        }

        let session_list_box = model.sessions.widget();
        let widgets = view_output!();

        let input_sender = sender.input_sender().clone();
        session_list_box.connect_row_activated(move |_, row| {
            if let Some(session_id) = SessionList::get_session_id_from_row(row) {
                let _ = input_sender.send(SessionListMsg::SessionSelected(session_id));
            }
        });

        if model.sessions.is_empty() {
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SessionListMsg::SetTools(tools) => {
                self.active_tools = tools.clone();
                self.all_tools_selected = tools.len() == Tool::ALL.len();
                self.reload_sessions();
            }
            SessionListMsg::SetSearchQuery(query) => {
                self.search_query = query;
                self.reload_sessions();
            }
            SessionListMsg::SessionSelected(id) => {
                let _ = sender.output(SessionListOutput::SessionSelected(id));
            }
            SessionListMsg::ResumeRequested(id, tool) => {
                let _ = sender.output(SessionListOutput::ResumeRequested(id, tool));
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if self.sessions.is_empty() {
            if !self.search_query.trim().is_empty() {
                widgets.empty_state.set_title("No sessions match search");
                widgets
                    .empty_state
                    .set_description(Some("Try a different query or adjust filters"));
            } else if self.all_tools_selected {
                widgets.empty_state.set_title("No Sessions Yet");
                widgets
                    .empty_state
                    .set_description(Some("Your AI coding sessions will appear here"));
            } else {
                widgets.empty_state.set_title("No sessions match filters");
                widgets
                    .empty_state
                    .set_description(Some("Try adjusting the tool filters in the sidebar"));
            }
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);
        }
    }
}

impl SessionList {
    const SESSION_ID_KEY: &'static str = "session-id";

    fn fetch_sessions(db_path: &Path, tools: &[Tool], query: &str) -> Vec<Session> {
        let query = query.trim();
        let sessions = if query.is_empty() {
            load_sessions(db_path, tools)
        } else {
            search_sessions(db_path, tools, query)
        };

        match sessions {
            Ok(sessions) => sessions,
            Err(err) => {
                tracing::error!("Failed to load sessions: {}", err);
                Vec::new()
            }
        }
    }

    fn get_session_id_from_row(row: &gtk::ListBoxRow) -> Option<String> {
        unsafe {
            row.data::<String>(Self::SESSION_ID_KEY)
                .map(|ptr| ptr.as_ref().clone())
        }
    }

    fn reload_sessions(&mut self) {
        let fetched = Self::fetch_sessions(&self.db_path, &self.active_tools, &self.search_query);
        let mut guard = self.sessions.guard();
        guard.clear();
        for session in fetched {
            guard.push_back(SessionRowInit { session });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib::prelude::ObjectExt;
    use relm4::Component;
    use relm4::ComponentController;
    use std::{cell::RefCell, rc::Rc};

    fn find_list_box(widget: &gtk::Widget) -> Option<gtk::ListBox> {
        if let Ok(list_box) = widget.clone().downcast::<gtk::ListBox>() {
            return Some(list_box);
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_list_box(&child_widget) {
                return Some(found);
            }
            child = child_widget.next_sibling();
        }

        None
    }

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        for _ in 0..50 {
            if condition() {
                break;
            }
            context.iteration(false);
        }
    }

    #[gtk::test]
    fn session_list_activates_on_single_click() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        assert!(list_box.activates_on_single_click());
    }

    #[gtk::test]
    fn session_list_emits_selection_on_row_activation() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let outputs: Rc<RefCell<Vec<SessionListOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = SessionList::builder()
            .launch(temp_db.path().to_path_buf())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        let session = Session {
            id: "test-session".to_string(),
            tool: Tool::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            start_time: chrono::Utc::now(),
            message_count: 1,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: chrono::Utc::now(),
        };

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: session.clone(),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        let row = list_box.row_at_index(0).expect("row");

        list_box.emit_by_name::<()>("row-activated", &[&row]);

        pump_main_context(|| !outputs.borrow().is_empty());

        let outputs = outputs.borrow();
        assert!(matches!(
            outputs.as_slice(),
            [SessionListOutput::SessionSelected(id)] if id == "test-session"
        ));
    }
}
