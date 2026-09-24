# 抖音 Lite

用 Rust + Tauri 2 包一层抖音网页版的桌面客户端。目标是去掉官方桌面客户端的臃肿部分，
保留常规导航（精选 / 推荐 / 关注 / 朋友 / 我的）和登录，再加上键盘流和下载。

## 它是怎么工作的

主窗口直接加载 `https://www.douyin.com/`，靠 `initialization_script` 注入
[`src-tauri/scripts/inject.js`](app/src-tauri/scripts/inject.js) 来做所有定制。

关键取舍：**不逆任何签名**。裸 HTTP 请求抖音只会拿到 `byted_acrawler` 挑战页，
而 WebView2 里抖音自己的 SDK 会把签名算好。所以注入脚本只 hook `fetch` / `XMLHttpRequest`
**旁听** feed 响应里的 `video.play_addr.url_list`，拿到地址后交给 Rust 下载。

这也意味着：抖音的 class 名是构建哈希（`wiu7QUYe` 这种），所有选择器只依赖
`data-e2e` / `data-e2e-vid` / `data-aweme-id` 这类语义属性。

## 数据落在哪

装在哪，资料就在哪 —— 一律相对 exe 所在目录，不往用户目录写：

- `<exe 目录>\data\webview\EBWebView\`：WebView2 用户数据，登录态、LocalStorage 和抖音视频媒体缓存都在这里。
  不重定向的话它默认落在 `%LOCALAPPDATA%\com.lite.douyin`，实测光缓存就能到 385 MB 且还在长。
- `<exe 目录>\Downloads\`：按 `D` 下载的视频，以及你在页面上点出来的下载（另存视频、抖音自己的下载按钮）。

两个目录启动时创建。把整个文件夹拷到别处，登录态跟着走。

## 快捷键

| 键 | 作用 |
|---|---|
| `↑` `↓` / `J` `K` / `N` `P` | 上一个 / 下一个视频 |
| `Space` | 暂停继续 |
| `M` | 静音 |
| `,` `.` `0` | 减速 / 加速 / 恢复 1x |
| `D` | 下载当前视频到「下载」目录 |
| `F` | 全屏 |
| `B` | 弹幕开关 |
| `Alt` `X` | 拾取模式：点掉任何你觉得臃肿的东西（规则存 localStorage） |
| `Alt` `Z` | 撤销所有手动隐藏 |
| `?` | 帮助 |

原生标题栏会通过 `DWMWA_CAPTION_COLOR` 跟随页面顶部背景色（需 Windows 11 22000+）。

## 构建

前置：Rust（MSVC 工具链）、Node 18+、WebView2 Runtime（Win10/11 自带）。

```bash
cd app
npm install
npx tauri build          # 产出 src-tauri/target/release/bundle/nsis/*.exe
```

开发时如果想连进页面调试，用调试端口启动，然后跑 `app/tools/cdp-check.mjs`：

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
  ./app/src-tauri/target/debug/douyin-lite.exe
node app/tools/cdp-check.mjs eval "window.__DYL.debug.currentId()"
node app/tools/cdp-check.mjs shot out.png
```

## 已知边界

- **省的是磁盘和体验，不省内存。** 页面是抖音原样给的，实测总占用约 1.9 GB
  （主进程 43 MB + WebView2 各子进程）。exe 本身 4.6 MB。
- 抖音改版会让注入脚本的部分选择器失效。脚本的设计是**失效时报错而不是猜一个**，
  所以最坏情况是某个功能提示不可用，不会静默下错视频。
- 下载依赖抖音返回的取流地址，域名白名单见
  [`download.rs`](app/src-tauri/src/download.rs)。

## 声明

非官方第三方客户端，与抖音/字节跳动无关，不隶属于官方，未获任何授权。
「抖音」商标与应用图标归字节跳动所有，本仓库只把它们用作个人构建的产品图标。
代码以 MIT 许可发布（见 [LICENSE](LICENSE)），仅供个人学习与研究。

自动化访问和下载可能违反抖音用户协议，账号风控风险自负，建议用小号。
若权利人认为本仓库存在侵权，请在 issue 里指明要求下架的范围，我会处理（删除相应内容或整仓）。
