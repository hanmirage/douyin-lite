use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

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
    /// 入库序号，只用于淘汰，不接收前端传值
    #[serde(skip_deserializing)]
    pub seq: u64,
}

/// 旁听窗口大小。超出后按 seq 淘汰最旧的，不再整表清空——
/// 清空会连你正在看那条的取流地址一起丢掉。
const MAX_ITEMS: usize = 800;

#[derive(Default)]
pub struct Store {
    items: Mutex<HashMap<String, MediaItem>>,
    next_seq: AtomicU64,
    user_agent: Mutex<String>,
}

/// 按 id 合并一批旁听结果，并在超出窗口时淘汰最旧的。
fn ingest(store: &mut HashMap<String, MediaItem>, items: Vec<MediaItem>, next_seq: &AtomicU64) {
    for mut item in items.into_iter().filter(|i| is_valid_id(&i.id)) {
        item.seq = next_seq.fetch_add(1, Ordering::Relaxed);
        let keep_known_urls = store
            .get(&item.id)
            .is_some_and(|known| !known.urls.is_empty() && item.urls.is_empty());
        if keep_known_urls {
            if let Some(known) = store.get_mut(&item.id) {
                known.seq = item.seq;
            }
        } else {
            store.insert(item.id.clone(), item);
        }
    }
    if store.len() > MAX_ITEMS {
        let mut by_seq: Vec<(u64, String)> = store.iter().map(|(k, v)| (v.seq, k.clone())).collect();
        by_seq.sort();
        for (_, id) in by_seq.into_iter().take(store.len() - MAX_ITEMS) {
            store.remove(&id);
        }
    }
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
    ingest(&mut store, items, &state.next_seq);
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

    let dir = crate::paths::download_dir()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, desc: &str, urls: &[&str]) -> MediaItem {
        MediaItem {
            id: id.to_string(),
            desc: desc.to_string(),
            author: String::new(),
            urls: urls.iter().map(|s| s.to_string()).collect(),
            seq: 0,
        }
    }

    fn seq_counter() -> AtomicU64 {
        AtomicU64::new(1)
    }

    #[test]
    fn whitelist_accepts_https_subdomains() {
        assert!(url_allowed("https://v26-web.douyinvod.com/a.mp4"));
        assert!(url_allowed("https://www.douyin.com/b.mp4"));
        assert!(url_allowed("https://douyinvod.com/c.mp4"));
    }

    #[test]
    fn whitelist_rejects_plain_http() {
        assert!(!url_allowed("http://v26-web.douyinvod.com/a.mp4"));
    }

    /// 后缀拼接式的伪装域名：`ends_with(".{domain}")` 单独用不够，必须整体解析
    #[test]
    fn whitelist_rejects_lookalike_hosts() {
        for u in [
            "https://evil-douyinvod.com/a.mp4",
            "https://douyinvod.com.evil.cn/a.mp4",
            "https://v26-web.douyinvod.com.attacker.net/a.mp4",
            "https://evil.com/?u=https://v26-web.douyinvod.com/a.mp4",
        ] {
            assert!(!url_allowed(u), "不该放行 {}", u);
        }
    }

    #[test]
    fn whitelist_rejects_unparsable() {
        assert!(!url_allowed(""));
        assert!(!url_allowed("blob:https://www.douyin.com/x"));
        assert!(!url_allowed("javascript:alert(1)"));
    }

    #[test]
    fn id_must_be_10_to_25_digits() {
        assert!(is_valid_id("7688933322579299603"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("123456789"));
        assert!(!is_valid_id("768893332257929960a"));
        assert!(!is_valid_id("76889333225792996031234567890"));
    }

    #[test]
    fn filename_strips_windows_illegal_chars() {
        let dirty = format!("a<b>:c/d{}|f?g*h{}i", '\\', '"');
        let name = safe_filename(&item("1234567890", &dirty, &[]));
        for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
            assert!(!name.contains(c), "文件名里不该留着 {}: {}", c, name);
        }
        assert_eq!(name, "abcdfghi.mp4", "实际 {}", name);
    }

    #[test]
    fn filename_falls_back_to_id_when_desc_empty() {
        assert_eq!(safe_filename(&item("7688933322579299603", "   ", &[])), "douyin-7688933322579299603.mp4");
    }

    #[test]
    fn filename_trims_trailing_dots() {
        assert_eq!(safe_filename(&item("1234567890", "完了...", &[])), "完了.mp4");
    }

    #[test]
    fn filename_caps_length_in_chars_not_bytes() {
        let long = "中".repeat(200);
        let name = safe_filename(&item("1234567890", &long, &[]));
        assert_eq!(name.chars().count(), 94, "实际 {}", name.chars().count());
        assert!(name.starts_with("中中中"));
        assert!(name.ends_with(".mp4"));
    }

    #[test]
    fn unique_path_appends_index_instead_of_overwriting() {
        let dir = std::env::temp_dir().join(format!("dylite-ut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("视频.mp4"), b"x").unwrap();
        let p = unique_path(&dir, "视频.mp4");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "视频 (2).mp4");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ingest_keeps_known_urls_when_new_batch_has_none() {
        let mut store = HashMap::new();
        let n = seq_counter();
        ingest(&mut store, vec![item("7688933322579299603", "有地址", &["https://v26-web.douyinvod.com/a.mp4"])], &n);
        ingest(&mut store, vec![item("7688933322579299603", "有地址", &[])], &n);
        assert_eq!(store["7688933322579299603"].urls.len(), 1, "空批次不该把已抓到的地址抹掉");
    }

    #[test]
    fn ingest_fills_urls_when_known_has_none() {
        let mut store = HashMap::new();
        let n = seq_counter();
        ingest(&mut store, vec![item("7688933322579299603", "先到 id", &[])], &n);
        ingest(&mut store, vec![item("7688933322579299603", "后到地址", &["https://v26-web.douyinvod.com/a.mp4"])], &n);
        assert_eq!(store["7688933322579299603"].urls.len(), 1);
        assert_eq!(store["7688933322579299603"].desc, "后到地址");
    }

    #[test]
    fn ingest_ignores_invalid_ids() {
        let mut store = HashMap::new();
        let n = seq_counter();
        ingest(&mut store, vec![item("not-an-id", "x", &["https://v26-web.douyinvod.com/a.mp4"])], &n);
        assert!(store.is_empty());
    }

    /// 这条就是原来那个缺陷：溢出时整表清空，会把正在看那条的地址一起丢掉
    #[test]
    fn overflow_evicts_oldest_and_keeps_current() {
        let mut store = HashMap::new();
        let n = seq_counter();
        for i in 0..MAX_ITEMS + 5 {
            ingest(&mut store, vec![item(&format!("{:019}", i), "d", &["https://v26-web.douyinvod.com/a.mp4"])], &n);
        }
        assert_eq!(store.len(), MAX_ITEMS, "应停在窗口上限");
        let newest = format!("{:019}", MAX_ITEMS + 4);
        assert!(store.contains_key(&newest), "最后一条必须在");
        assert!(!store.contains_key("0000000000000000000"), "最旧的应被淘汰");
    }
}
