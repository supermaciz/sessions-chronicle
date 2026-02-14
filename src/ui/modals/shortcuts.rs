use adw::gtk::prelude::GtkApplicationExt;
use adw::prelude::AdwDialogExt;
use relm4::adw;
use relm4::prelude::*;

pub struct ShortcutsDialog;

impl SimpleComponent for ShortcutsDialog {
    type Root = adw::ShortcutsDialog;
    type Widgets = adw::ShortcutsDialog;
    type Init = ();
    type Input = ();
    type Output = ();

    fn init_root() -> Self::Root {
        adw::ShortcutsDialog::builder().build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {};
        let widgets = root.clone();

        // General section
        let general = adw::ShortcutsSection::new(Some("General"));
        general.add(adw::ShortcutsItem::new(
            "Keyboard Shortcuts",
            "<Control>question",
        ));
        general.add(adw::ShortcutsItem::new("Preferences", "<Control>comma"));
        general.add(adw::ShortcutsItem::new("Quit", "<Control>q"));
        widgets.add(general);

        // Search section
        let search = adw::ShortcutsSection::new(Some("Search"));
        search.add(adw::ShortcutsItem::new("Search", "<Control>f"));
        widgets.add(search);

        // View section
        let view = adw::ShortcutsSection::new(Some("View"));
        view.add(adw::ShortcutsItem::new("Toggle utility pane", "F9"));
        widgets.add(view);

        widgets.present(Some(&relm4::main_adw_application().windows()[0]));
        ComponentParts { model, widgets }
    }
}
