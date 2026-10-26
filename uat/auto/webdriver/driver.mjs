// A minimal W3C WebDriver client for the real app, through tauri-driver and
// the msedgedriver that matches the installed WebView2. It speaks the
// protocol with fetch, so it needs no package.
//
// The app runs on a throwaway Windows profile: USERPROFILE, APPDATA and
// LOCALAPPDATA point into a folder named cd-throwaway-profile, so the app sees
// none of the tester's Agent homes and writes its own data there. Both AppData
// folders must sit inside USERPROFILE, or Tauri can't resolve its data folder
// and the session fails with "DevToolsActivePort file doesn't exist"
// (docs/research/uat-automation.md, section 2).
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

export function profileEnv(profile) {
  if (basename(profile) !== "cd-throwaway-profile") {
    throw new Error(`the profile folder must be named cd-throwaway-profile, not ${profile}`);
  }
  const roaming = join(profile, "AppData", "Roaming");
  const local = join(profile, "AppData", "Local");
  // The Windows folder picker opens on a known folder, and says "Ubicación no
  // disponible" when the profile lacks it.
  for (const folder of [roaming, local, ...["Desktop", "Documents", "Downloads"].map((f) => join(profile, f))]) {
    mkdirSync(folder, { recursive: true });
  }
  // An empty .claude.json stands for the tester's own Claude Code home, so
  // Claude Code has two homes once 1.2 adds the throwaway one, as 2.3 needs.
  const claude = join(profile, ".claude.json");
  if (!existsSync(claude)) writeFileSync(claude, "{}\n");
  return { USERPROFILE: profile, APPDATA: roaming, LOCALAPPDATA: local, HOME: profile };
}

// Starts tauri-driver on `port` with the app's environment and waits until it
// answers. Returns the child process.
export async function startTauriDriver({ nativeDriver, env, port = 4444 }) {
  const child = spawn("tauri-driver", ["--port", String(port), "--native-driver", nativeDriver], {
    env: { ...process.env, ...env },
    stdio: "ignore",
    windowsHide: true,
  });
  let failed;
  child.once("error", (e) => (failed = e));
  const end = Date.now() + 15000;
  for (;;) {
    if (failed) throw new Error(`tauri-driver did not start: ${failed.message}`);
    try {
      const res = await fetch(`http://127.0.0.1:${port}/status`);
      if (res.ok) return child;
    } catch {}
    if (Date.now() > end) {
      child.kill();
      throw new Error("tauri-driver did not answer on /status");
    }
    await sleep(200);
  }
}

export class Session {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async open({ app, port = 4444 }) {
    const base = `http://127.0.0.1:${port}`;
    const value = await call(base, "POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
    });
    return new Session(base, value.sessionId);
  }

  static attach(id, port = 4444) {
    return new Session(`http://127.0.0.1:${port}`, id);
  }

  cmd(method, path, body) {
    return call(this.base, method, `/session/${this.id}${path}`, body);
  }

  close() {
    return this.cmd("DELETE", "");
  }

  // Runs `script` in the page. `args` reach it as `arguments`; pass an
  // element as ref(el).
  exec(script, ...args) {
    return this.cmd("POST", "/execute/sync", { script, args });
  }

  async find(css, root) {
    const path = root ? `/element/${root}/element` : "/element";
    const value = await this.cmd("POST", path, { using: "css selector", value: css });
    return value[ELEMENT];
  }

  // Waits until `probe` returns something truthy, and returns it.
  async until(probe, { timeout = 15000, what = "a condition" } = {}) {
    const end = Date.now() + timeout;
    let last;
    for (;;) {
      try {
        const value = await probe();
        if (value) return value;
      } catch (e) {
        last = e;
      }
      if (Date.now() > end) throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ""}`);
      await sleep(100);
    }
  }

  // The first element matching `css` whose text contains `text`, or null.
  byText(css, text) {
    return this.exec(
      `return [...document.querySelectorAll(arguments[0])].find((e) => e.textContent.includes(arguments[1])) ?? null;`,
      css,
      text,
    ).then((v) => (v ? v[ELEMENT] : null));
  }

  waitText(css, text, opts) {
    return this.until(() => this.byText(css, text), { what: `${css} with "${text}"`, ...opts });
  }

  waitFor(css, opts) {
    return this.until(() => this.find(css).catch(() => null), { what: css, ...opts });
  }

  // Clicks as the tester does. When a notice covers the element, the tester
  // scrolls it to the middle or waits for the notice to go: so does this,
  // with the mouse moved off the notice, which stays while hovered.
  async click(el) {
    for (let attempt = 0; ; attempt++) {
      try {
        return await this.cmd("POST", `/element/${el}/click`, {});
      } catch (e) {
        if (!/element click intercepted/.test(e.message) || attempt === 3) throw e;
        await this.hover({ x: 2, y: 2 });
        await this.exec("arguments[0].scrollIntoView({ block: 'center' });", { [ELEMENT]: el });
        if (attempt > 0) await sleep(2500);
      }
    }
  }

  type(el, text) {
    return this.cmd("POST", `/element/${el}/value`, { text });
  }

  text(el) {
    return this.cmd("GET", `/element/${el}/text`);
  }

  attr(el, name) {
    return this.cmd("GET", `/element/${el}/attribute/${name}`);
  }

  rect(el) {
    return this.cmd("GET", `/element/${el}/rect`);
  }

  // The page's visible text, as the tester reads it.
  bodyText() {
    return this.exec("return document.body.innerText;");
  }

  setWindow(width, height) {
    return this.cmd("POST", "/window/rect", { width, height });
  }

  // Key presses through the Actions API, so focus and default actions behave
  // as with a real keyboard. Keys are WebDriver code points, such as KEY.Tab.
  async keys(...keys) {
    const actions = [];
    for (const k of keys) {
      if (Array.isArray(k)) {
        for (const m of k) actions.push({ type: "keyDown", value: m });
        for (const m of [...k].reverse()) actions.push({ type: "keyUp", value: m });
      } else {
        for (const ch of [...k]) actions.push({ type: "keyDown", value: ch }, { type: "keyUp", value: ch });
      }
    }
    await this.cmd("POST", "/actions", { actions: [{ type: "key", id: "kb", actions }] });
    await this.cmd("DELETE", "/actions");
  }

  // Moves the mouse onto the centre of `el`, or onto `x`, `y` of the
  // viewport, and leaves it there.
  async hover(target) {
    const move =
      typeof target === "string"
        ? { type: "pointerMove", duration: 50, origin: { [ELEMENT]: target }, x: 0, y: 0 }
        : { type: "pointerMove", duration: 50, origin: "viewport", x: target.x, y: target.y };
    await this.cmd("POST", "/actions", {
      actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" }, actions: [move] }],
    });
  }

  // Writes a PNG of the window. Screenshots stay on this machine: they are
  // never committed, attached or uploaded (docs/agents/screenshots.md).
  async screenshot(file) {
    const b64 = await this.cmd("GET", "/screenshot");
    writeFileSync(file, Buffer.from(b64, "base64"));
    return file;
  }

  // Runs a Chrome DevTools Protocol command on the webview, through
  // msedgedriver's own endpoint, which tauri-driver passes on.
  cdp(cmd, params = {}) {
    return this.cmd("POST", "/ms/cdp/execute", { cmd, params });
  }

  // Installs the walk's instruments in this page and in every page the
  // webview loads after, such as on a reload. See INSTRUMENTS.
  async instrument() {
    await this.cdp("Page.addScriptToEvaluateOnNewDocument", { source: INSTRUMENTS });
    await this.exec(INSTRUMENTS);
  }

  // Holds the answer of each named command for its number of milliseconds,
  // so a busy state that lasts less than a frame on this machine stays long
  // enough to check. `{}` lifts every delay. A delay outlives a reload.
  delay(delays) {
    return this.exec("window.__uatSetDelays(arguments[0]);", delays);
  }

  // Loads the page again, as when the app opens: the first reads run anew.
  reload() {
    return this.cdp("Page.reload", { ignoreCache: true });
  }
}

// Tauri's invoke can't be replaced (its property is neither writable nor
// configurable), but on Windows it sends every command with fetch to
// http://ipc.localhost/<command>. The instruments wrap fetch to hold a
// command's answer, and keep the delays in sessionStorage so a reload keeps
// them.
function instruments() {
  if (window.__uatSetDelays) return;
  const read = () => {
    try {
      return JSON.parse(sessionStorage.getItem("__uatDelays") || "{}");
    } catch {
      return {};
    }
  };
  let delays = read();
  window.__uatSetDelays = (next) => {
    delays = next;
    try {
      sessionStorage.setItem("__uatDelays", JSON.stringify(next));
    } catch {}
  };
  // Errors the page throws or logs, kept across a reload, so a step that
  // finds the window blank can say why.
  const keep = (text) => {
    try {
      const list = JSON.parse(sessionStorage.getItem("__uatErrors") || "[]");
      list.push(String(text).slice(0, 2000));
      sessionStorage.setItem("__uatErrors", JSON.stringify(list.slice(-20)));
    } catch {}
  };
  window.addEventListener("error", (e) => keep(`${e.message} ${e.error?.stack ?? ""}`));
  window.addEventListener("unhandledrejection", (e) => keep(`unhandled: ${e.reason?.stack ?? e.reason}`));
  const error = console.error;
  console.error = (...args) => {
    keep(args.map((a) => a?.stack ?? (typeof a === "string" ? a : JSON.stringify(a))).join(" "));
    error(...args);
  };
  const fetch = window.fetch;
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input.url;
    const match = /^https?:\/\/ipc\.localhost\/([^?/]+)/.exec(url);
    const ms = match && delays[decodeURIComponent(match[1])];
    if (ms) await new Promise((r) => setTimeout(r, ms));
    return fetch(input, init);
  };
}
const INSTRUMENTS = `(${instruments})();`;

export const KEY = {
  Tab: "\uE004",
  Enter: "\uE007",
  Escape: "\uE00C",
  Space: " ",
  Backspace: "\uE003",
  Up: "\uE013",
  Down: "\uE015",
  Alt: "\uE00A",
  Shift: "\uE008",
  Control: "\uE009",
  // Lifts every modifier an earlier key of the same text holds down.
  Null: "\uE000",
};

// An element found by `find`, as an argument of `exec`.
export const ref = (el) => ({ [ELEMENT]: el });

// The element a script returned, as `find` gives it, or null.
export const unwrap = (value) => (value ? value[ELEMENT] : null);

async function call(base, method, path, body) {
  const res = await fetch(base + path, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${json.value?.message ?? JSON.stringify(json)}`);
  return json.value;
}
