use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, SimpleComponent,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application,
};

use adw::prelude::AdwApplicationWindowExt;
use gtk::prelude::{
    ApplicationExt, ButtonExt, GtkWindowExt, OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt,
};
use gtk::{gio, glib};

use crate::config::{APP_ID, PROFILE};
use crate::ui::modals::{about::AboutDialog, shortcuts::ShortcutsDialog};
use crate::ui::{session_list::SessionList, sidebar::Sidebar};

pub(super) struct App {
    search_visible: bool,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    ToggleSearch,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Widgets = AppWidgets;

    menu! {
        primary_menu: {
            section! {
                "_Preferences" => PreferencesAction,
                "_Keyboard" => ShortcutsAction,
                "_About Sessions Chronicle" => AboutAction,
            }
        }
    }

    view! {
        main_window = adw::ApplicationWindow::new(&main_application()) {
            set_visible: true,

            connect_close_request[sender] => move |_| {
                sender.input(AppMsg::Quit);
                glib::Propagation::Stop
            },

            add_css_class?: if PROFILE == "Devel" {
                    Some("devel")
                } else {
                    None
                },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start = &gtk::ToggleButton {
                        set_icon_name: "system-search-symbolic",
                        set_tooltip_text: Some("Search sessions"),
                        #[watch]
                        set_active: model.search_visible,
                        connect_toggled[sender] => move |_| {
                            sender.input(AppMsg::ToggleSearch);
                        },
                    },

                    pack_end = &gtk::MenuButton {
                        set_icon_name: "open-menu-symbolic",
                        set_menu_model: Some(&primary_menu),
                    },
                },

                #[wrap(Some)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    #[name = "search_bar"]
                    gtk::SearchBar {
                        #[watch]
                        set_search_mode: model.search_visible,

                        #[wrap(Some)]
                        set_child = &gtk::SearchEntry {
                            set_placeholder_text: Some("Search sessions..."),
                            set_hexpand: true,
                        },
                    },

                    adw::NavigationSplitView {
                        set_vexpand: true,

                        #[wrap(Some)]
                        set_sidebar = &adw::NavigationPage::builder()
                            .title("Filters")
                            .child(sidebar.widget())
                            .build(),

                        #[wrap(Some)]
                        set_content = &adw::NavigationPage::builder()
                            .title("Sessions")
                            .child(session_list.widget())
                            .build(),
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Initialize child components
        let sidebar = Sidebar::builder().launch(()).detach();
        let session_list = SessionList::builder().launch(()).detach();

        let model = Self {
            search_visible: false,
        };
        let widgets = view_output!();

        let app = root.application().unwrap();
        let mut actions = RelmActionGroup::<WindowActionGroup>::new();

        let shortcuts_action = {
            RelmAction::<ShortcutsAction>::new_stateless(move |_| {
                ShortcutsDialog::builder().launch(()).detach();
            })
        };

        let about_action = {
            RelmAction::<AboutAction>::new_stateless(move |_| {
                AboutDialog::builder().launch(()).detach();
            })
        };

        let quit_action = {
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        // Connect action with hotkeys
        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);

        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(quit_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::ToggleSearch => {
                self.search_visible = !self.search_visible;
            }
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
    }
}

impl AppWidgets {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(APP_ID);
        let (width, height) = self.main_window.default_size();

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;

        settings.set_boolean("is-maximized", self.main_window.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(APP_ID);

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let is_maximized = settings.boolean("is-maximized");

        self.main_window.set_default_size(width, height);

        if is_maximized {
            self.main_window.maximize();
        }
    }
}
