use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

const TRAY_ID: &str = "splitwave";
pub const SHOW_ID: &str = "tray_show";
pub const QUIT_ID: &str = "tray_quit";
pub const PIPELINE_ID_PREFIX: &str = "tray_pipeline:";
pub const TOGGLE_EVENT: &str = "tray://toggle_pipeline";
const EMPTY_ID: &str = "tray_no_pipelines";

pub struct Pipeline {
    pub id: String,
    pub name: String,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let menu = build_menu(app, &[], None)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Splitwave")
        .build(app)?;
    Ok(())
}

pub fn sync(
    app: &AppHandle,
    pipelines: &[Pipeline],
    running_id: Option<&str>,
) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    tray.set_menu(Some(build_menu(app, pipelines, running_id)?))
}

fn build_menu(
    app: &AppHandle,
    pipelines: &[Pipeline],
    running_id: Option<&str>,
) -> tauri::Result<tauri::menu::Menu<Wry>> {
    let show = MenuItemBuilder::with_id(SHOW_ID, "Show Splitwave").build(app)?;
    let quit = MenuItemBuilder::with_id(QUIT_ID, "Quit Splitwave").build(app)?;

    let heading = MenuItemBuilder::with_id("tray_pipelines_heading", "Pipelines")
        .enabled(false)
        .build(app)?;
    let mut menu = MenuBuilder::new(app).item(&heading);
    if pipelines.is_empty() {
        let empty = MenuItemBuilder::with_id(EMPTY_ID, "No pipelines")
            .enabled(false)
            .build(app)?;
        menu = menu.item(&empty);
    } else {
        for pipeline in pipelines {
            let item = CheckMenuItemBuilder::with_id(
                format!("{PIPELINE_ID_PREFIX}{}", pipeline.id),
                pipeline.name.as_str(),
            )
            .checked(running_id == Some(pipeline.id.as_str()))
            .build(app)?;
            menu = menu.item(&item);
        }
    }
    menu.separator().item(&show).item(&quit).build()
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
