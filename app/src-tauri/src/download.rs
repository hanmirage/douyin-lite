use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};

/// 注入脚本从 feed 响应里旁听到的视频信息。
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Default)]
pub struct Store {
    items: Mutex<HashMap<String, MediaItem>>,
    user_agent: Mutex<String>,
}

/// 远程页面可以调用本模块的命令，所以取流地址必须落在已知 CDN 域名内，
/// 否则 dy_download 就是一个任意 URL 抓取器。
const ALLOWED_HOSTS: &[&str] = &[
    "douyin.com",
    "douyinvod.com",
    "douyinstatic.com",
    "douyinpic.com",
    "pstatp.com",
    "snssdk.com",
    "bytetos.com",
    "bytecdn.cn",
    "toutiaovod.com",
    "bdxiguaimg.com",
    "ixigua.com",
    "zjcdn.com",
    "volcfcdndvs.com",
];

fn url_allowed(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    ALLOWED_HOSTS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn is_valid_id(id: &str) -> bool {
    (10..=25).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

fn safe_filename(item: &MediaItem) -> String {
    let desc = item.desc.trim();
    let author = item.author.trim();
    let raw = if desc.is_empty() {
        format!("douyin-{id}", id = item.id)
    } else if author.is_empty() {
        desc.to_string()
    } else {
        format!("{author} - {desc}")
    };

    let mut cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control())
        .collect();
    cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() > 90 {
        cleaned = cleaned.chars().take(90).collect();
    }
    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }
    if cleaned.is_empty() {
        cleaned = format!("douyin-{id}", id = item.id);
    }
    format!("{cleaned}.mp4")
}

/// 同名文件不覆盖，追加序号。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    for i in 2..1000 {
        let path = dir.join(format!("{stem} ({i}).mp4"));
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-{}.mp4", std::time::UNIX_EPOCH.elapsed().map_or(0, |d| d.as_secs())))
}

#[tauri::command]
pub fn dy_ingest(
    state: State<'_, Store>,
    items: Vec<MediaItem>,
    ua: Option<String>,
) -> Result<usize, String> {
    if let Some(ua) = ua.filter(|v| !v.is_empty() && v.len() < 400) {
        *state.user_agent.lock().map_err(|_| "内部状态不可用")? = ua;
    }
    let mut store = state.items.lock().map_err(|_| "内部状态不可用")?;
    if store.len() > 800 {
        store.clear();
    }
    for item in items.into_iter().filter(|i| is_valid_id(&i.id)) {
        store
            .entry(item.id.clone())
            .and_modify(|known| {
                if known.urls.is_empty() {
                    known.urls = item.urls.clone();
                }
            })
            .or_insert(item);
    }
    Ok(store.len())
}

#[tauri::command]
pub async fn dy_download(app: AppHandle, state: State<'_, Store>, id: String) -> Result<String, String> {
    let id = id.trim().to_string();
    if !is_valid_id(&id) {
        return Err("视频 ID 不合法".into());
    }

    let item = {
        let store = state.items.lock().map_err(|_| "内部状态不可用")?;
        store.get(&id).cloned()
    }
    .ok_or("还没抓到这个视频的取流地址，等它开始播放后再按 D")?;

    let url = item
        .urls
        .iter()
        .find(|u| url_allowed(u))
        .ok_or_else(|| {
            if item.urls.is_empty() {
                "这条内容没有视频取流地址（可能是图文或直播）".to_string()
            } else {
                format!("取流地址域名不在白名单内，已拒绝下载：{}", item.urls[0])
            }
        })?;

    let dir = app
        .path()
        .resolve("", BaseDirectory::Download)
        .map_err(|e| e.to_string())?;
    let ua = {
        let guard = state.user_agent.lock().map_err(|_| "内部状态不可用")?;
        if guard.is_empty() {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0".to_string()
        } else {
            guard.clone()
        }
    };

    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(url.as_str())
        .header(reqwest::header::REFERER, "https://www.douyin.com/")
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("服务端返回 {}", resp.status()));
    }

    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = unique_path(&dir, &safe_filename(&item));

    // 边下边写：合集类视频能到几百 MB，不能整块读进内存。
    let mut file = fs::File::create(&target).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            let _ = fs::remove_file(&target);
            format!("读取视频流失败: {e}")
        })?;
        file.write_all(&chunk).map_err(|e| {
            let _ = fs::remove_file(&target);
            e.to_string()
        })?;
        written += chunk.len() as u64;
    }
    file.flush().map_err(|e| e.to_string())?;
    if written < 20 * 1024 {
        let _ = fs::remove_file(&target);
        return Err(format!("只拿到 {written} 字节，像是错误页而不是视频"));
    }

    let path = target.display().to_string();
    let _ = app.emit("dy:downloaded", path.clone());
    Ok(path)
}
