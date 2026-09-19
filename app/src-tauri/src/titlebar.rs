//! 让原生标题栏跟随页面顶部背景色。
//!
//! 走 DWM 的 DWMWA_CAPTION_COLOR（Windows 11 22000+）而不是无边框自绘控件：
//! 保留系统的最小化/最大化/贴边分屏，且失败模式只是"颜色不对"，不会把登录按钮盖住。

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_COLOR_DEFAULT: u32 = 0xFFFFFFFF;

use tauri::Manager;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: *mut core::ffi::c_void,
        attribute: u32,
        value: *const u32,
        size_of_value: u32,
    ) -> i32;
}

/// COLORREF 的字节序是 0x00BBGGRR。
fn colorref(rgb: (u8, u8, u8)) -> u32 {
    let (r, g, b) = rgb;
    ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

/// 亮度低的标题栏要用浅色按钮（— □ ✕），否则反过来。
fn is_dark(rgb: (u8, u8, u8)) -> bool {
    let (r, g, b) = rgb;
    0.299 * (r as f64) + 0.587 * (g as f64) + 0.114 * (b as f64) < 140.0
}

fn apply(hwnd: *mut core::ffi::c_void, rgb: Option<(u8, u8, u8)>) {
    let dark: u32 = match rgb {
        Some(v) => u32::from(is_dark(v)),
        None => 1,
    };
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, 4);
    }
    let caption = rgb.map_or(DWMWA_COLOR_DEFAULT, colorref);
    let border = rgb.map_or(DWMWA_COLOR_DEFAULT, colorref);
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &caption, 4);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border, 4);
    }
}

/// 窗口刚建好、注入脚本还没报颜色时先按深色处理，避免白标题栏闪一下。
pub fn init_dark(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if let Ok(hwnd) = win.hwnd() {
            apply(hwnd.0, None);
        }
    }
}

#[tauri::command]
pub fn dy_set_titlebar(app: tauri::AppHandle, r: u8, g: u8, b: u8) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("没有主窗口")?;
    let hwnd = win.hwnd().map_err(|e| e.to_string())?;
    apply(hwnd.0, Some((r, g, b)));
    Ok(())
}
