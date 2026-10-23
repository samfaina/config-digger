// What a step does in the app, in the words of the plan: open a screen, a
// tab, press a button, choose an option. Each helper finds its element by
// what the tester reads, not by a class name, where the app gives a role or a
// label to find it by.
import { ref } from "./driver.mjs";

export const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// The rail's screens, by the tooltip the tester sees on each button.
export async function go(s, screen) {
  const el = await s.waitFor(`.rail button[aria-label='${screen}']`);
  await s.click(el);
  await wait(300);
}

export async function tab(s, name) {
  const el = await s.waitText("[role=tab]", name);
  await s.click(el);
  await wait(200);
}

// A visible, enabled button whose own text starts with `text`, ignoring the
// case that CSS gives it, inside the element matching `scope` when given.
export function buttonEl(s, text, scope = "body") {
  return s
    .exec(
      `const want = arguments[1].toLowerCase();
       const scope = [...document.querySelectorAll(arguments[0])].at(-1);
       if (!scope) return null;
       return [...scope.querySelectorAll("button, [role=button]")].find((b) =>
         b.offsetParent !== null && (b.textContent.trim().toLowerCase().startsWith(want) || (b.getAttribute("aria-label") ?? "").toLowerCase() === want)
       ) ?? null;`,
      scope,
      text,
    )
    .then((v) => (v ? Object.values(v)[0] : null));
}

export async function press(s, text, scope) {
  const el = await s.until(() => buttonEl(s, text, scope), { what: `button "${text}"${scope ? ` in ${scope}` : ""}` });
  await s.until(async () => (await s.attr(el, "disabled")) === null, { what: `button "${text}" enabled` });
  await s.click(el);
  await wait(150);
  return el;
}

// Picks `option` in the combobox labelled `label`, the last one on the page
// when a dialog sits over the screen.
export async function choose(s, label, option) {
  const field = await s.until(
    () =>
      s
        .exec(`return [...document.querySelectorAll("[role=combobox][aria-label='" + arguments[0] + "']")].at(-1) ?? null;`, label)
        .then((v) => (v ? Object.values(v)[0] : null)),
    { what: `the ${label} field` },
  );
  await s.click(field);
  const opt = await s.until(
    () =>
      s
        .exec(
          `return [...document.querySelectorAll("[role=option]")].find((o) => o.textContent.trim() === arguments[0]) ??
                  [...document.querySelectorAll("[role=option]")].find((o) => o.textContent.includes(arguments[0])) ?? null;`,
          option,
        )
        .then((v) => (v ? Object.values(v)[0] : null)),
    { what: `option "${option}" of ${label}` },
  );
  await s.exec("arguments[0].scrollIntoView({block: 'nearest'})", ref(opt));
  await s.click(opt);
  await wait(200);
  return field;
}

export function fieldValue(s, label) {
  return s.exec(
    `const f = [...document.querySelectorAll("[role=combobox][aria-label='" + arguments[0] + "']")].at(-1);
     return f ? f.textContent.trim() : null;`,
    label,
  );
}

// Replaces a field's text with key presses, Ctrl+A and Backspace first, so
// React sees each change: WebDriver's clear sends no input event.
export async function typeInto(s, css, text) {
  const el = await s.waitFor(css);
  await s.type(el, "a");
  if (text) await s.type(el, text);
  return el;
}

// The texts of the notices at the bottom of the screen, with their kind.
export function notices(s) {
  return s.exec(
    `return [...document.querySelectorAll(".notice, .snackbar")].map((n) => ({ cls: n.className, text: n.textContent.trim() }));`,
  );
}

export function waitNotice(s, text, opts) {
  return s.until(
    async () => (await notices(s)).find((n) => n.text.includes(text)),
    { what: `the notice "${text}"`, ...opts },
  );
}

// Starts a recorder that keeps, with its time, every notice, busy notice,
// loading block and read bar that appears, so a step can check what showed
// for less than a second after the fact.
export function record(s) {
  return s.exec(`
    const seen = (window.__uatSeen = []);
    const t0 = performance.now();
    const look = () => {
      for (const el of document.querySelectorAll(".notice, .snackbar, .loading, .read-progress, .dialog-title, .card-reading, [role=progressbar]")) {
        const text = (el.getAttribute("aria-label") ?? "") + "|" + el.textContent.trim().slice(0, 200);
        const key = el.className + "|" + text;
        if (!seen.some((e) => e.key === key && e.live)) seen.push({ key, cls: el.className, text, t: Math.round(performance.now() - t0), live: true });
      }
      for (const e of seen) e.live = e.live && [...document.querySelectorAll("*")].length > 0;
    };
    if (window.__uatObserver) window.__uatObserver.disconnect();
    window.__uatObserver = new MutationObserver(look);
    window.__uatObserver.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["class"] });
    look();
  `);
}

export function seen(s) {
  return s.exec("return (window.__uatSeen ?? []).map(({ cls, text, t }) => ({ cls, text, t }));");
}

// Moves the mouse off to a quiet spot and then onto `el`, as the tester
// does: a tooltip opens on the mouse entering, which a mouse already there,
// say after a click, doesn't do again.
export async function hoverFresh(s, el) {
  await s.hover({ x: 600, y: 4 });
  await wait(150);
  await s.hover(el);
}

// The app's tooltip on screen, with its box and look, or null.
export function tooltip(s) {
  return s.exec(`
    const t = [...document.querySelectorAll(".tooltip[role=tooltip]")].find((e) => !e.hidden && getComputedStyle(e).visibility !== "hidden" && getComputedStyle(e).display !== "none");
    if (!t) return null;
    const r = t.getBoundingClientRect(), c = getComputedStyle(t);
    const range = document.createRange(); range.selectNodeContents(t);
    const lines = new Set([...range.getClientRects()].filter((x) => x.width > 0).map((x) => Math.round(x.top))).size;
    return { text: t.textContent, rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, vw: innerWidth, vh: innerHeight, lines,
      bg: c.backgroundColor, radius: c.borderTopLeftRadius, border: c.borderTopWidth + " " + c.borderTopStyle, shadow: c.boxShadow };`);
}

// The open dropdown panel: its options, the active and chosen ones, its
// group titles, its search box and its box.
export function panel(s) {
  return s.exec(`
    const p = document.querySelector(".select-panel");
    if (!p) return null;
    const r = p.getBoundingClientRect(), c = getComputedStyle(p);
    const opt = (o) => ({ text: o.textContent.trim(), active: o.classList.contains("active"), chosen: o.classList.contains("chosen"), disabled: o.getAttribute("aria-disabled") === "true", color: getComputedStyle(o).color, bg: getComputedStyle(o).backgroundColor });
    const search = p.querySelector("input");
    return {
      options: [...p.querySelectorAll("[role=option]")].map(opt),
      groups: [...p.querySelectorAll(".select-group")].map((g) => g.innerText.trim()),
      empty: p.querySelector(".select-empty")?.textContent.trim() ?? null,
      search: search && { value: search.value, placeholder: search.placeholder, focused: document.activeElement === search },
      rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, vw: innerWidth, vh: innerHeight,
      bg: c.backgroundColor, radius: c.borderTopLeftRadius, shadow: c.boxShadow,
      scrolls: (() => { const l = p.querySelector(".select-list"); return l ? l.scrollHeight > l.clientHeight + 1 : false; })(),
    };`);
}

// The focused element: its role, label and outline or border colour.
export function focused(s) {
  return s.exec(`
    const e = document.activeElement;
    if (!e || e === document.body) return null;
    const c = getComputedStyle(e);
    return { role: e.getAttribute("role"), label: e.getAttribute("aria-label"), text: e.textContent.trim().slice(0, 80), tag: e.tagName,
      expanded: e.getAttribute("aria-expanded"), outline: c.outlineStyle + " " + c.outlineColor, border: c.borderTopColor, shadow: c.boxShadow };`);
}

// Presses Tab until `match` holds for the focused element, at most `max` times.
export async function tabTo(s, match, max = 40) {
  for (let i = 0; i < max; i++) {
    const f = await focused(s);
    if (f && match(f)) return f;
    await s.keys("");
  }
  return null;
}

// The window's focus event, as when the tester comes back to config-digger
// from another window. WebDriver can't move the focus to another app, so the
// step sends the event the app listens to.
export function refocus(s) {
  return s.exec("window.dispatchEvent(new FocusEvent('focus'));");
}

// The row of a Project or an Agent home, by the path it shows.
export function placeRow(s, path) {
  return s
    .exec(
      `return [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]) ?? null;`,
      path,
    )
    .then((v) => (v ? Object.values(v)[0] : null));
}

export async function rowButton(s, path, text) {
  return s.exec(
    `const row = [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]);
     if (!row) return null;
     return [...row.querySelectorAll("button")].find((b) => b.textContent.trim().toLowerCase().startsWith(arguments[1].toLowerCase())) ?? null;`,
    path,
    text,
  ).then((v) => (v ? Object.values(v)[0] : null));
}
