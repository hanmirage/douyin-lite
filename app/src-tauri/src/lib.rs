mod download;
mod titlebar;

use tauri::webview::PageLoadEvent;
use tauri::{WebviewUrl, WebviewWindowBuilder};

const START_URL: &str = "https://www.douyin.com/?recommend=1";
const INIT_SCRIPT: &str = include_str!("../scripts/inject.js");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(download::Store::default())
        .invoke_handler(tauri::generate_handler![
            download::dy_ingest,
            download::dy_download,
            titlebar::dy_set_titlebar
        ])
        .setup(|app| {
            let url = tauri::Url::parse(START_URL).map_err(|e| e.to_string())?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("抖音 Lite")
                .initialization_script(INIT_SCRIPT)
                .inner_size(1180.0, 800.0)
                .min_inner_size(720.0, 520.0)
                .center()
                // 白屏期先藏起来，首屏加载完再显示
                .visible(false)
                .on_page_load(|webview, payload| {
                    if matches!(payload.event(), PageLoadEvent::Finished) {
                        let _ = webview.show();
                        let _ = webview.set_focus();
                    }
                })
                .build()
                .map_err(|e| e.to_string())?;
            titlebar::init_dark(&app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("抖音 Lite 启动失败");
}
