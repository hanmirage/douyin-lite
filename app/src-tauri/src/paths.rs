//! 落盘位置一律从 exe 所在目录推出来：装在哪，资料就在哪，不往用户目录写。

use std::path::PathBuf;

fn exe_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("拿不到 exe 所在目录")?;
    Ok(dir.to_path_buf())
}

/// WebView2 用户数据目录。登录态、LocalStorage 和媒体缓存都在里面，
/// 默认会落到 %LOCALAPPDATA%\com.lite.douyin，抖音的视频缓存能长到几百 MB。
pub fn webview_data_dir() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join("data").join("webview"))
}

/// 视频落盘目录。按 D 下载的与网页自身发起的下载共用这一个。
pub fn download_dir() -> Result<PathBuf, String> {
    Ok(exe_dir()?.join("Downloads"))
}
