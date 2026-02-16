use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk;

use crate::database::load_message_full_content;
use crate::models::{MessagePreview, Role};
use crate::ui::highlight;
use crate::ui::markdown;

pub struct MessageRowInit {
    pub preview: MessagePreview,
    pub highlight_query: Option<String>,
    pub db_path: Arc<PathBuf>,
}

#[derive(Debug)]
pub enum MessageRowMsg {
    ToggleExpand,
}

#[derive(Debug)]
pub enum MessageRowCmd {
    FullContentLoaded(Result<String>),
}

#[derive(Debug)]
pub enum MessageRowOutput {
    MatchCountChanged {
        message_index: usize,
        count: usize,
    },
    #[allow(dead_code)]
    ExpandLoadFailed {
        message_index: usize,
    },
}

#[derive(Debug)]
pub struct MessageRow {
    preview: MessagePreview,
    highlight_query: Option<String>,
    db_path: Arc<PathBuf>,
    expanded: bool,
    full_content: Option<String>,
    loading_full_content: bool,
    rendered_match_count: usize,
}

/// Render content into the given container, returning the highlight match count.
fn render_content(
    container: &gtk::Box,
    content: &str,
    role: Role,
    highlight_query: Option<&str>,
) -> usize {
    // Clear existing children
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let mut match_count = 0usize;

    if role == Role::Assistant {
        let rendered = markdown::render_markdown(content, highlight_query);
        match_count = rendered.1;
        container.append(&rendered.0);
    } else if let Some(query) = highlight_query {
        let (markup, count) = highlight::highlight_text(content, query);
        match_count = count;
        let label = gtk::Label::new(None);
        label.set_markup(&markup);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(true);
        container.append(&label);
    } else {
        let label = gtk::Label::new(Some(content));
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(true);
        container.append(&label);
    }

    match_count
}

#[relm4::factory(pub)]
impl FactoryComponent for MessageRow {
    type Init = MessageRowInit;
    type Input = MessageRowMsg;
    type Output = MessageRowOutput;
    type CommandOutput = MessageRowCmd;
    type ParentWidget = gtk::Box;

    view! {
        root = gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 4,
            add_css_class: "message-row",
            add_css_class: self.preview.role.css_class(),

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 8,

                gtk::Label {
                    set_label: self.preview.role.label(),
                    add_css_class: "caption",
                    add_css_class: "heading",
                    add_css_class: self.preview.role.css_class(),
                    set_halign: gtk::Align::Start,
                },

                gtk::Label {
                    set_label: &self.preview.timestamp.format("%H:%M:%S").to_string(),
                    add_css_class: "caption",
                    add_css_class: "dim-label",
                    set_halign: gtk::Align::Start,
                },
            },

            #[name(content_container)]
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 4,
            },

            // Expand/collapse toggle button
            gtk::Button {
                #[watch]
                set_label: &if self.loading_full_content {
                    "Loading...".to_string()
                } else if self.expanded {
                    "Collapse".to_string()
                } else {
                    "Show full message".to_string()
                },
                add_css_class: "flat",
                add_css_class: "caption",
                add_css_class: "expand-toggle",
                set_halign: gtk::Align::Start,
                set_margin_top: 4,
                #[watch]
                set_sensitive: !self.loading_full_content,
                #[watch]
                set_visible: self.preview.is_truncated() && self.preview.role != Role::ToolResult,
                connect_clicked => MessageRowMsg::ToggleExpand,
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            preview: init.preview,
            highlight_query: init.highlight_query,
            db_path: init.db_path,
            expanded: false,
            full_content: None,
            loading_full_content: false,
            rendered_match_count: 0,
        }
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        _root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let widgets = view_output!();

        let match_count = render_content(
            &widgets.content_container,
            &self.preview.content_preview,
            self.preview.role,
            self.highlight_query.as_deref(),
        );
        self.rendered_match_count = match_count;

        sender
            .output(MessageRowOutput::MatchCountChanged {
                message_index: self.preview.message_index,
                count: match_count,
            })
            .ok();

        widgets
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::Input,
        sender: FactorySender<Self>,
    ) {
        match message {
            MessageRowMsg::ToggleExpand => {
                if self.expanded {
                    // Collapse: re-render preview content
                    self.expanded = false;
                    let count = render_content(
                        &widgets.content_container,
                        &self.preview.content_preview,
                        self.preview.role,
                        self.highlight_query.as_deref(),
                    );
                    if count != self.rendered_match_count {
                        self.rendered_match_count = count;
                        sender
                            .output(MessageRowOutput::MatchCountChanged {
                                message_index: self.preview.message_index,
                                count,
                            })
                            .ok();
                    }
                } else if let Some(ref full) = self.full_content {
                    // Expand with cached content
                    self.expanded = true;
                    let count = render_content(
                        &widgets.content_container,
                        full,
                        self.preview.role,
                        self.highlight_query.as_deref(),
                    );
                    if count != self.rendered_match_count {
                        self.rendered_match_count = count;
                        sender
                            .output(MessageRowOutput::MatchCountChanged {
                                message_index: self.preview.message_index,
                                count,
                            })
                            .ok();
                    }
                } else {
                    // Fetch full content from DB in background
                    self.loading_full_content = true;
                    let db_path = self.db_path.clone();
                    let session_id = self.preview.session_id.clone();
                    let message_index = self.preview.message_index;
                    sender.spawn_oneshot_command(move || {
                        MessageRowCmd::FullContentLoaded(load_message_full_content(
                            &db_path,
                            &session_id,
                            message_index,
                        ))
                    });
                }
            }
        }
        self.update_view(widgets, sender);
    }

    fn update_cmd_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        message: Self::CommandOutput,
        sender: FactorySender<Self>,
    ) {
        match message {
            MessageRowCmd::FullContentLoaded(Ok(content)) => {
                self.full_content = Some(content.clone());
                self.expanded = true;
                self.loading_full_content = false;
                let count = render_content(
                    &widgets.content_container,
                    &content,
                    self.preview.role,
                    self.highlight_query.as_deref(),
                );
                if count != self.rendered_match_count {
                    self.rendered_match_count = count;
                    sender
                        .output(MessageRowOutput::MatchCountChanged {
                            message_index: self.preview.message_index,
                            count,
                        })
                        .ok();
                }
            }
            MessageRowCmd::FullContentLoaded(Err(err)) => {
                tracing::error!(
                    "Failed to load full content for message {}: {}",
                    self.preview.message_index,
                    err
                );
                self.expanded = false;
                self.loading_full_content = false;
                sender
                    .output(MessageRowOutput::ExpandLoadFailed {
                        message_index: self.preview.message_index,
                    })
                    .ok();
            }
        }
        self.update_view(widgets, sender);
    }
}
