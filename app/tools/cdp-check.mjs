// 通过 WebView2 的远程调试端口，连进抖音 Lite 的页面内部取证。
// 用法：
//   node tools/cdp-check.mjs targets
//   node tools/cdp-check.mjs eval "window.__DYL?.booted"
//   node tools/cdp-check.mjs shot C:\temp\app.png
const BASE = process.env.CDP || "http://127.0.0.1:9222";
const [mode = "targets", arg = ""] = process.argv.slice(2);

const list = await (await fetch(BASE + "/json/list")).json().catch((e) => {
  console.log("连不上调试端口，确认用 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 启动:", e.message);
  process.exit(2);
});

const page = list.find((t) => /douyin\.com/.test(t.url || "") && t.webSocketDebuggerUrl);
if (mode === "targets") {
  console.log(JSON.stringify(list.map((t) => ({ type: t.type, url: (t.url || "").slice(0, 110), title: (t.title || "").slice(0, 40) })), null, 1));
  process.exit(0);
}
if (!page) {
  console.log("没有找到 douyin.com 页面目标，现有目标:", list.map((t) => t.url));
  process.exit(3);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
const pending = new Map();
let seq = 0;
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  } else if (mode === "console" && msg.method === "Runtime.consoleAPICalled") {
    console.log("[page]", msg.args.map((a) => a.value ?? a.description ?? a.type).join(" "));
  } else if (mode === "console" && msg.method === "Runtime.exceptionThrown") {
    console.log("[error]", msg.params.exceptionDetails.exception?.description || msg.params.exceptionDetails.text);
  }
};
await new Promise((res, rej) => {
  ws.onopen = res;
  ws.onerror = () => rej(new Error("CDP websocket 连接失败"));
});
const send = (method, params = {}) =>
  new Promise((res) => {
    const id = ++seq;
    pending.set(id, res);
    ws.send(JSON.stringify({ id, method, params }));
  });

await send("Runtime.enable");

if (mode === "eval") {
  const r = await send("Runtime.evaluate", { expression: arg, returnByValue: true, awaitPromise: true });
  if (r.result?.exceptionDetails) console.log("抛错:", r.result.exceptionDetails.exception?.description);
  else console.log(typeof r.result?.result?.value === "object" ? JSON.stringify(r.result.result.value, null, 1) : String(r.result?.result?.value));
} else if (mode === "shot") {
  await send("Page.enable");
  const r = await send("Page.captureScreenshot", { format: "png" });
  const out = arg || "cdp-shot.png";
  const { writeFileSync } = await import("node:fs");
  writeFileSync(out, Buffer.from(r.result.data, "base64"));
  console.log("截图已写入", out);
} else if (mode === "console") {
  console.log("监听页面日志中，Ctrl+C 退出");
  await new Promise(() => {});
}

ws.close();
process.exit(0);
