// 抖音 Lite 注入脚本：document-start 阶段跑在主框架。
// 选择器只依赖 data-e2e / data-aweme-id / id 这类语义属性，
// 因为抖音的 class 名是构建哈希（wiu7QUYe 这种），一改版就全废。
(() => {
  "use strict";
  if (window.top !== window) return;
  const host = location.hostname;
  if (host !== "douyin.com" && !host.endsWith(".douyin.com")) return;

  const S = (window.__DYL = window.__DYL || {});
  if (S.booted) return;
  S.booted = true;
  S.ua = navigator.userAgent;
  S.seen = new Map();
  // 只装「上次上报之后新增或补全」的条目，避免每轮把全量重发给 Rust。
  S.outbox = new Map();
  // JS 侧只做窗口，真正的下载凭据在 Rust 的 store 里，所以这里可以淘汰最旧的。
  S.cap = 400;

  const HIDE_KEY = "dylite:hidden";
  const ok = (v) => typeof v === "string" && v;

  /* ---------------------------------- 样式 ---------------------------------- */

  const BASE_CSS = `
    [data-e2e="danmaku-container"]:not([data-dyl-show]) { display: none !important; }
    [data-e2e="header-on-top"], [data-e2e="header-minimize"],
    [data-e2e="header-maximize"], [data-e2e="header-close"] { display: none !important; }
    #a11y-open-btn { display: none !important; }
    [data-dyl-pick="1"] { cursor: crosshair !important; outline: 1px dashed #ff3b5c !important; }
    #dyl-toast, #dyl-help { font-family: system-ui, "Microsoft YaHei", sans-serif; }
    #dyl-toast {
      position: fixed; z-index: 2147483646; right: 18px; bottom: 18px; max-width: 46vw;
      padding: 9px 13px; border-radius: 8px; font-size: 13px; line-height: 1.5;
      color: #fff; background: rgba(18,18,22,.93); border: 1px solid rgba(255,255,255,.14);
      box-shadow: 0 6px 24px rgba(0,0,0,.4); opacity: 0; transform: translateY(6px);
      transition: opacity .18s, transform .18s; pointer-events: none; word-break: break-all;
    }
    #dyl-toast[data-on="1"] { opacity: 1; transform: none; }
  `;

  function userCss() {
    try {
      return JSON.parse(localStorage.getItem(HIDE_KEY) || "[]")
        .map((sel) => `${sel}{display:none!important}`)
        .join("\n");
    } catch {
      return "";
    }
  }

  function applyCss() {
    const root = document.head || document.documentElement;
    if (!root) return;
    let el = document.getElementById("dylite-style");
    if (!el) {
      el = document.createElement("style");
      el.id = "dylite-style";
      root.appendChild(el);
    }
    const css = BASE_CSS + "\n" + userCss();
    // 重写 textContent 会让整页样式失效重算，内容没变就别动
    if (el.textContent !== css) el.textContent = css;
  }

  applyCss();
  // document-start 时 head 可能还不存在，补一次
  document.addEventListener("readystatechange", applyCss, { once: true });

  let toastTimer;
  function toast(msg, ms = 2600) {
    const root = document.body || document.documentElement;
    if (!root) return;
    let el = document.getElementById("dyl-toast");
    if (!el) {
      el = document.createElement("div");
      el.id = "dyl-toast";
      root.appendChild(el);
    }
    el.textContent = msg;
    el.dataset.on = "1";
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (el.dataset.on = "0"), ms);
  }

  /* --------------------------------- 旁听 API --------------------------------- */

  function harvest(node, depth) {
    if (!node || typeof node !== "object" || depth > 8) return;
    if (Array.isArray(node)) {
      for (const item of node) harvest(item, depth + 1);
      return;
    }
    const info = node.aweme_info || node.awemeInfo || node;
    const id = info.aweme_id || info.awemeId || info.aweme_id_str;
    if (ok(id)) {
      const video = info.video || info.player || {};
      const play = video.play_addr || video.playAddr || {};
      const urls = []
        .concat(play.url_list || play.urlList || [])
        .concat(video.playApi || video.decode_uri || [])
        .filter(ok);
      const author = info.author || {};
      const entry = {
        id: String(id),
        desc: ok(info.desc) ? info.desc.slice(0, 120) : "",
        author: ok(author.nickname) ? author.nickname : ok(author.unique_id) ? author.unique_id : "",
        urls,
      };
      const known = S.seen.get(entry.id);
      if (!known || (known.urls.length === 0 && entry.urls.length)) {
        S.seen.set(entry.id, entry);
        S.outbox.set(entry.id, entry);
        if (S.seen.size > S.cap) S.seen.delete(S.seen.keys().next().value);
      }
    }
    for (const key in node) {
      if (key === "children" || key === "_owner") continue;
      harvest(node[key], depth + 1);
    }
  }

  const looksLikeFeed = (url) =>
    /\/aweme\/v1\/web\/|\/aweme\/v1\/play|module\/feed|feed\/v\d|general\/search|aweme\/detail|multi\/aweme/i.test(url);

  function sniffJson(url, payload) {
    if (!payload || typeof payload !== "object") return;
    try {
      harvest(payload, 0);
    } catch (err) {
      console.debug("[dylite] harvest", err);
    }
    if (S.outbox.size) flush();
  }

  const nativeFetch = window.fetch;
  if (typeof nativeFetch === "function") {
    window.fetch = function (input, init) {
      const url = typeof input === "string" ? input : (input && input.url) || "";
      const p = nativeFetch.apply(this, arguments);
      if (looksLikeFeed(url)) {
        p.then((res) => {
          try {
            res.clone().json().then((j) => sniffJson(url, j)).catch(() => {});
          } catch {}
        });
      }
      return p;
    };
  }

  const NativeXHR = window.XMLHttpRequest;
  if (NativeXHR) {
    const open = NativeXHR.prototype.open;
    const send = NativeXHR.prototype.send;
    NativeXHR.prototype.open = function (method, url) {
      this.__dylUrl = String(url || "");
      return open.apply(this, arguments);
    };
    NativeXHR.prototype.send = function () {
      const xhr = this;
      if (looksLikeFeed(xhr.__dylUrl || "")) {
        xhr.addEventListener("load", () => {
          try {
            if (/json/.test(xhr.getResponseHeader("content-type") || "")) {
              sniffJson(xhr.__dylUrl, JSON.parse(xhr.responseText));
            }
          } catch {}
        });
      }
      return send.apply(this, arguments);
    };
  }

  /* ----------------------------------- IPC ----------------------------------- */

  let flushTimer;
  function flush() {
    clearTimeout(flushTimer);
    flushTimer = setTimeout(push, 900);
  }

  function push() {
    const tauri = window.__TAURI__;
    const batch = [...S.outbox.values()];
    if (!batch.length || !tauri || !tauri.core) return;
    S.outbox.clear();
    tauri.core
      .invoke("dy_ingest", { items: batch, ua: S.ua })
      .catch((e) => {
        // 报不进去就退回去，下一轮再试；丢了 Rust 侧就没有这条的取流地址
        console.debug("[dylite] ingest 失败", e);
        for (const it of batch) S.outbox.set(it.id, it);
      });
  }

  async function invoke(cmd, args) {
    const tauri = window.__TAURI__;
    if (!tauri || !tauri.core) throw new Error("IPC 未就绪（远程页 capability 没配好）");
    return tauri.core.invoke(cmd, args);
  }

  function currentId() {
    // 沉浸式推荐流的权威 id 挂在 #sliderVideo 的 data-e2e-vid 上，会随切播更新。
    const slider = document.querySelector('[data-e2e="feed-active-video"][data-e2e-vid]');
    if (slider) return slider.getAttribute("data-e2e-vid");

    const fromUrl = location.pathname.match(/\/video\/(\d{10,25})/) || location.search.match(/modal_id=(\d{10,25})/);
    if (fromUrl) return fromUrl[1];

    // 精选这类网格页里，每个卡片自带 data-aweme-id
    const playing = [...document.querySelectorAll("video")].find((v) => v.readyState > 2 && !v.paused);
    const holder = playing && playing.closest("[data-aweme-id]");
    if (holder) return holder.getAttribute("data-aweme-id");

    const centre = document.elementFromPoint(Math.round(innerWidth / 2), Math.round(innerHeight / 2));
    const near = centre && centre.closest("[data-aweme-id]");
    return near ? near.getAttribute("data-aweme-id") : null;
  }

  async function download() {
    const id = currentId();
    if (!id) return toast("没识别到当前视频（页面结构变了？），悬停在画面上再按 D");
    // 不在本地缓存里也照样问一次 Rust：JS 侧会淘汰旧的，Rust 侧才是下载的凭据。
    toast("开始下载…", 1600);
    try {
      const path = await invoke("dy_download", { id });
      toast("已保存 " + path, 6000);
    } catch (e) {
      toast("下载失败：" + (e && e.message ? e.message : e), 5200);
    }
  }

  function media() {
    return [...document.querySelectorAll("video")].find((v) => v.readyState > 0) || document.querySelector("video");
  }

  function switchTo(dir) {
    // 推荐流是 transform 驱动的虚拟列表，程序化 scrollTop / 合成滚轮事件都不生效，
    // 只有页面自己的切播按钮可靠（已实测）。
    // 精选网格里同名箭头有几十份且大多在视口外，必须挑可见的那一个。
    const sel = dir > 0 ? '[data-e2e="video-switch-next-arrow"]' : '[data-e2e="video-switch-prev-arrow"]';
    const btn = [...document.querySelectorAll(sel)].find((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && r.top >= 0 && r.bottom <= innerHeight;
    });
    if (!btn) return false;
    btn.click();
    return true;
  }

  function rate(step) {
    const v = media();
    if (!v) return toast("当前没有视频");
    const next = Math.min(4, Math.max(0.25, Math.round((v.playbackRate + step) * 100) / 100));
    v.playbackRate = next;
    toast(`倍速 ${next}x`);
  }

  function mute() {
    const v = media();
    if (!v) return toast("当前没有视频");
    v.muted = !v.muted;
    toast(v.muted ? "静音" : "取消静音");
  }

  function toggleDanmaku() {
    const el = document.querySelector('[data-e2e="danmaku-container"]');
    if (!el) return toast("这个页面没有弹幕层");
    if (el.dataset.dylShow === "1") delete el.dataset.dylShow;
    else el.dataset.dylShow = "1";
    // 规则是静态的 `:not([data-dyl-show])`，改 dataset 就够了，不必重写整张样式表
    toast(el.dataset.dylShow === "1" ? "显示弹幕" : "隐藏弹幕");
  }

  function fullscreen() {
    const btn = document.querySelector('[data-e2e="xgplayer-page-full-screen"]');
    if (btn) return btn.click();
    if (document.fullscreenElement) document.exitFullscreen();
    else document.documentElement.requestFullscreen?.();
  }

  /* ------------------------------ 自选隐藏（拾取器） ------------------------------ */

  function selectorOf(el) {
    const e2e = el.getAttribute("data-e2e") || el.getAttribute("data-e2e-creator");
    if (e2e) return `[data-e2e="${e2e}"]`;
    if (el.id) return `#${CSS.escape(el.id)}`;
    const path = [];
    let n = el;
    while (n && n !== document.body) {
      const parent = n.parentElement;
      if (!parent) break;
      path.unshift(`${n.tagName.toLowerCase()}:nth-child(${[...parent.children].indexOf(n) + 1})`);
      n = parent;
    }
    return "body>" + path.join(">");
  }

  function picking(on) {
    const rules = readRules();
    if (on) {
      document.documentElement.dataset.dylPick = "1";
      document.addEventListener("click", grab, true);
      toast("拾取模式：点击要干掉的东西，Esc 退出");
    } else {
      delete document.documentElement.dataset.dylPick;
      document.removeEventListener("click", grab, true);
      toast(`已隐藏 ${rules.length} 处${rules.length ? "（Alt+Z 全部撤销）" : ""}`);
    }
  }

  function readRules() {
    try {
      return JSON.parse(localStorage.getItem(HIDE_KEY) || "[]");
    } catch {
      return [];
    }
  }

  function grab(ev) {
    if (document.documentElement.dataset.dylPick !== "1") return;
    ev.preventDefault();
    ev.stopPropagation();
    const target = ev.target;
    if (!(target instanceof Element) || target.id === "dyl-toast") return;
    const sel = selectorOf(target);
    const rules = readRules();
    if (!rules.includes(sel)) rules.push(sel);
    localStorage.setItem(HIDE_KEY, JSON.stringify(rules));
    applyCss();
    toast("已隐藏 " + sel.slice(0, 70));
  }

  function clearHidden() {
    localStorage.removeItem(HIDE_KEY);
    applyCss();
    toast("已恢复所有隐藏项");
  }

  /* --------------------------------- 帮助面板 --------------------------------- */

  const HELP = `
    ↑ ↓ / J K / N P   上一个、下一个视频
    Space             暂停 / 继续
    M                 静音        , .  减速 / 加速      0  恢复 1x
    D                 下载当前视频到「下载」目录
    F                 全屏        B  弹幕开关
    Alt+X             拾取模式：点掉任何你觉得臃肿的东西
    Alt+Z             撤销所有手动隐藏
    Esc               关闭本面板
  `;

  function help() {
    const old = document.getElementById("dyl-help");
    if (old) return old.remove();
    const box = document.createElement("pre");
    box.id = "dyl-help";
    box.textContent = HELP;
    Object.assign(box.style, {
      position: "fixed", zIndex: 2147483647, left: "50%", top: "16%", transform: "translateX(-50%)",
      padding: "18px 22px", borderRadius: "12px", margin: 0, fontSize: "13px", lineHeight: "1.9",
      color: "#eee", background: "rgba(12,12,16,.96)", border: "1px solid rgba(255,255,255,.14)",
      boxShadow: "0 12px 48px rgba(0,0,0,.5)", whiteSpace: "pre-wrap",
    });
    (document.body || document.documentElement).appendChild(box);
    setTimeout(() => box.remove(), 15000);
  }

  /* --------------------------------- 键盘分流 --------------------------------- */

  document.addEventListener(
    "keydown",
    (ev) => {
      const t = ev.target;
      const typing =
        t instanceof HTMLElement &&
        (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
      if (ev.key === "Escape") {
        if (document.documentElement.dataset.dylPick === "1") picking(false);
        return;
      }
      if (typing) return;
      if (ev.altKey && !ev.ctrlKey && !ev.metaKey) {
        const k = ev.key.toLowerCase();
        if (k === "x") {
          ev.preventDefault();
          picking(true);
        } else if (k === "z") {
          ev.preventDefault();
          clearHidden();
        }
        return;
      }
      if (ev.ctrlKey || ev.metaKey) return;

      const k = ev.key;
      const lower = typeof k === "string" ? k.toLowerCase() : "";
      let hit = true;
      if (k === "ArrowDown" || lower === "n" || lower === "j") {
        if (!switchTo(1)) toast("这个页面没有切播按钮，请用滚轮翻页");
      } else if (k === "ArrowUp" || lower === "p" || lower === "k") {
        if (!switchTo(-1)) toast("这个页面没有切播按钮，请用滚轮翻页");
      } else if (lower === "d") {
        download();
      } else if (lower === "m") {
        mute();
      } else if (lower === "b") {
        toggleDanmaku();
      } else if (lower === "f") {
        fullscreen();
      } else if (k === ",") {
        rate(-0.25);
      } else if (k === ".") {
        rate(0.25);
      } else if (k === "0") {
        const v = media();
        if (v) {
          v.playbackRate = 1;
          toast("倍速 1x");
        }
      } else if (k === "?" || (k === "/" && ev.shiftKey)) {
        help();
      } else if (k === " ") {
        const v = media();
        if (v) {
          v.paused ? v.play() : v.pause();
        } else {
          hit = false;
        }
      } else {
        hit = false;
      }
      if (hit) {
        ev.preventDefault();
        ev.stopPropagation();
      }
    },
    true,
  );

  /* -------------------------------- 标题栏跟随颜色 -------------------------------- */

  // 取页面顶部实际可见的背景色：从几个采样点往上找第一个不透明背景祖先。
  function topColor() {
    const xs = [0.18, 0.5, 0.85];
    for (const fx of xs) {
      let node = document.elementFromPoint(Math.round(innerWidth * fx), 3);
      for (let i = 0; i < 8 && node; i++) {
        const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/.exec(getComputedStyle(node).backgroundColor);
        if (m && (m[4] === undefined || Number(m[4]) > 0.5)) {
          return [Number(m[1]), Number(m[2]), Number(m[3])];
        }
        node = node.parentElement;
      }
    }
    return null;
  }

  let lastCaption = null;
  function syncCaption() {
    if (document.hidden) return;
    const c = topColor();
    if (!c) return;
    const prev = lastCaption;
    lastCaption = c;
    // 抖动太小的变化不值得发一次 IPC
    if (prev && c.every((v, i) => Math.abs(v - prev[i]) < 6)) return;
    invoke("dy_set_titlebar", { r: c[0], g: c[1], b: c[2] }).catch(() => {});
  }

  setInterval(syncCaption, 700);
  document.addEventListener("visibilitychange", () => {
    lastCaption = null;
    syncCaption();
  });

  /* ---------------------------------- 事件回显 ---------------------------------- */

  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen("dy:downloaded", (e) => toast("下载完成 " + e.payload, 7000)).catch(() => {});
  }

  // 有积压就补报一次：抖有的页面导航会把请求节奏打断，靠这个兜住
  setInterval(() => {
    if (S.outbox.size) flush();
  }, 4000);

  toast("抖音 Lite 已接管：? 看快捷键", 2200);

  // 暴露内部状态，便于用 CDP / 控制台核对注入是否生效
  S.debug = { currentId, selectorOf, seenSize: () => S.seen.size, hiddenRules: readRules };
})();
