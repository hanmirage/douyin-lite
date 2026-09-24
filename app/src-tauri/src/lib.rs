mod download;
mod paths;
mod titlebar;

use tauri::webview::{DownloadEvent, PageLoadEvent};
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
            let data_dir = paths::webview_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(paths::download_dir()?)?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("抖音 Lite")
                .initialization_script(INIT_SCRIPT)
                .inner_size(1180.0, 800.0)
                .min_inner_size(720.0, 520.0)
                .center()
                .data_directory(data_dir)
                // 用户在页面上点来的下载（另存视频/头像），跟 dy_download 落同一个目录
                .on_download(|_webview, event| {
                    if let DownloadEvent::Requested { destination, .. } = event {
                        if let Some(name) = destination.file_name().map(|n| n.to_os_string()) {
                            if let Ok(dir) = paths::download_dir() {
                                *destination = dir.join(name);
                            }
                        }
                    }
                    true
                })
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
