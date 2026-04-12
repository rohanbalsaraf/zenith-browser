use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

pub struct AppMenu {
    pub menu_bar: Menu,
    pub m_new_tab: MenuItem,
    pub m_new_incognito_tab: MenuItem,
    pub m_close_tab: MenuItem,
    pub m_bookmark: MenuItem,
    pub m_settings: MenuItem,
    pub m_find: MenuItem,
    pub m_history: MenuItem,
    pub m_downloads: MenuItem,
    pub m_theme: CheckMenuItem,
    pub m_reload: MenuItem,
    pub img_save: MenuItem,
    pub img_open: MenuItem,
    pub m_inspect: MenuItem,
    pub dots_menu: Menu,
    pub img_menu: Menu,
}

impl AppMenu {
    pub fn new(current_theme: &str) -> Self {
        let menu_bar = Menu::new();

        // Application Menu
        #[cfg(target_os = "macos")]
        {
            let app_menu = Submenu::new("Zenith", true);
            app_menu
                .append_items(&[
                    &PredefinedMenuItem::about(None, None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::services(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::hide(None),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::show_all(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::quit(None),
                ])
                .unwrap();
            menu_bar.append_items(&[&app_menu]).unwrap();
        }

        // File/Tab Menu
        let tab_menu = Submenu::new("Tabs", true);
        let m_new_tab = MenuItem::new(
            "New Tab",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyT)),
        );
        let m_new_incognito_tab = MenuItem::new(
            "New Incognito Tab",
            true,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::KeyN,
            )),
        );
        let m_close_tab = MenuItem::new(
            "Close Tab",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyW)),
        );
        let m_bookmark = MenuItem::new(
            "Bookmark Page",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyD)),
        );
        let m_settings = MenuItem::new(
            "Settings",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)),
        );
        tab_menu
            .append_items(&[
                &m_new_tab,
                &m_new_incognito_tab,
                &m_close_tab,
                &PredefinedMenuItem::separator(),
                &m_bookmark,
                &m_settings,
            ])
            .unwrap();

        // Edit/Find Menu
        let edit_menu = Submenu::new("Edit", true);
        let m_find = MenuItem::new(
            "Find in Page...",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyF)),
        );
        edit_menu
            .append_items(&[
                &PredefinedMenuItem::undo(None),
                &PredefinedMenuItem::redo(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
                &PredefinedMenuItem::separator(),
                &m_find,
            ])
            .unwrap();

        // View Menu
        let view_menu = Submenu::new("View", true);
        let m_history = MenuItem::new(
            "History",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyY)),
        );
        let m_downloads = MenuItem::new(
            "Downloads",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyJ)),
        );
        let m_theme = CheckMenuItem::new("Light Mode", true, current_theme == "light", None);
        let m_reload = MenuItem::new(
            "Reload Page",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyR)),
        );
        let m_inspect = MenuItem::new(
            "Inspect Element",
            true,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::ALT),
                Code::KeyI,
            )),
        );
        view_menu
            .append_items(&[
                &m_reload,
                &PredefinedMenuItem::separator(),
                &m_history,
                &m_downloads,
                &PredefinedMenuItem::separator(),
                &m_theme,
                &PredefinedMenuItem::separator(),
                &m_inspect,
            ])
            .unwrap();

        menu_bar
            .append_items(&[&tab_menu, &edit_menu, &view_menu])
            .unwrap();

        // Context menu (for dots button - shared items)
        let dots_menu = Menu::new();
        dots_menu
            .append_items(&[
                &m_new_tab,
                &m_new_incognito_tab,
                &m_bookmark,
                &m_history,
                &m_downloads,
                &muda::PredefinedMenuItem::separator(),
                &m_find,
                &m_theme,
                &muda::PredefinedMenuItem::separator(),
                &m_inspect,
                &muda::PredefinedMenuItem::separator(),
                &m_close_tab,
            ])
            .unwrap();

        // Image right-click context menu
        let img_menu = Menu::new();
        let img_save = MenuItem::new("Save Image to Downloads", true, None);
        let img_open = MenuItem::new("Open Image in New Tab", true, None);
        img_menu.append_items(&[&img_save, &img_open]).unwrap();

        Self {
            menu_bar,
            m_new_tab,
            m_new_incognito_tab,
            m_close_tab,
            m_bookmark,
            m_settings,
            m_find,
            m_history,
            m_downloads,
            m_theme,
            m_reload,
            img_save,
            img_open,
            m_inspect,
            dots_menu,
            img_menu,
        }
    }

    pub fn init(&self) {
        #[cfg(target_os = "macos")]
        self.menu_bar.init_for_nsapp();
    }
}
