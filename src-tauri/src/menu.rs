use crate::commands::shell_toggle_devtools;
use tauri::menu::{MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::{App, Manager};

const MENU_RELOAD: &str = "shell-menu-reload";
const MENU_DEVTOOLS: &str = "shell-menu-devtools";

pub fn setup_app_menu(app: &App, app_menu_title: &str, show_dev_menu: bool) -> Result<(), String> {
    let reload = MenuItem::with_id(app, MENU_RELOAD, "Reload", true, Some("CmdOrCtrl+Shift+R"))
        .map_err(|e| format!("create reload menu item: {e}"))?;

    let edit_submenu = SubmenuBuilder::new(app, "Edit")
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()
        .map_err(|e| format!("build edit menu: {e}"))?;

    let view_submenu = {
        let mut builder = SubmenuBuilder::new(app, "View").item(&reload);

        if show_dev_menu {
            let devtools = MenuItem::with_id(
                app,
                MENU_DEVTOOLS,
                "Open DevTools",
                true,
                Some("CmdOrCtrl+Shift+M"),
            )
            .map_err(|e| format!("create devtools menu item: {e}"))?;
            builder = builder.item(&devtools);
        }

        builder
            .build()
            .map_err(|e| format!("build view menu: {e}"))?
    };

    let app_submenu = SubmenuBuilder::new(app, app_menu_title)
        .quit()
        .build()
        .map_err(|e| format!("build app menu: {e}"))?;

    let menu = MenuBuilder::new(app)
        .item(&app_submenu)
        .item(&edit_submenu)
        .item(&view_submenu)
        .build()
        .map_err(|e| format!("build app menu bar: {e}"))?;

    app.set_menu(menu)
        .map_err(|e| format!("set app menu: {e}"))?;

    app.on_menu_event(|app, event| match event.id().as_ref() {
        MENU_RELOAD => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.reload();
            }
        }
        MENU_DEVTOOLS => {
            if let Err(error) = shell_toggle_devtools(app.clone()) {
                eprintln!("devtools menu: {error}");
            }
        }
        _ => {}
    });

    Ok(())
}
