fn main() {
    // 远程来源（douyin.com）的 IPC 一定会走 ACL 强制检查，所以必须把自定义命令
    // 登记进 app manifest，否则注入脚本调用时会得到 "not allowed. Plugin not found"。
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "dy_ingest",
                "dy_download",
                "dy_set_titlebar",
            ])),
    )
    .expect("tauri-build 生成 ACL 失败");
}
