// Section 1: the throwaway data, and the Agent homes added through the app.
import { press } from "../lib.mjs";
import { addHome, inventoryOf, openItem, project } from "./common.mjs";

export default [
  {
    id: "1.1",
    async run(t) {
      const out = t.setup();
      t.has(out, `Created the throwaway homes and Projects in ${t.root}`, `el script acaba con «Created the throwaway homes and Projects in ${t.root}»`);
      for (const file of ["claude-code-home\\.claude.json", "codex-home\\config.toml", "claude-desktop-home\\claude_desktop_config.json", "claude-code-shape-home\\.claude.json"]) {
        t.has(out, file, `lista «${file}»`);
      }
    },
  },
  {
    id: "1.2",
    async run(t) {
      const home = `${t.root}\\claude-code-home`;
      await addHome(t, "Claude Code", home);
      const text = await inventoryOf(t, "Claude Code", "claude-code-home");
      t.has(text, "cd-throwaway-user", "el Inventario del home muestra el servidor «cd-throwaway-user»");
      t.has(text, "cd-throwaway-user-skill", "muestra la skill «cd-throwaway-user-skill»");
      const details = await openItem(t, "cd-throwaway-user");
      t.check(!details.includes("ghp_not-a-real-secret") && !details.includes("not-a-real-secret"), "el Secret sale enmascarado, nunca «ghp_not-a-real-secret»");
      t.check(details.includes("••••"), "el Secret enmascarado sale como «••••»");
    },
  },
  {
    id: "1.3",
    async run(t) {
      const { s } = t;
      const home = `${t.root}\\claude-code-shape-home`;
      const file = `${home}\\.claude.json`;
      await addHome(t, "Claude Code", home, { checks: false });
      await inventoryOf(t, "Claude Code", "claude-code-shape-home");
      await project(t, `${t.root}\\project`);
      const text = await s.exec("return document.querySelector('.panel.items').innerText");
      t.has(text, "cd-throwaway-shape-user", "el Inventario del home muestra el servidor «cd-throwaway-shape-user»");
      // The notice's own words, without its View button.
      const notice = await s.exec(`
        const a = [...document.querySelectorAll(".panel.items .alert")].find((e) => e.textContent.includes("couldn't be read"));
        return a ? [...a.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE).map((n) => n.textContent).join("").trim() : null;`);
      t.eq(notice, "1 location couldn't be read.", "el aviso de arriba dice «1 location couldn't be read.»");
      const details = await openItem(t, "cd-throwaway-shape-user");
      const caveat = `A location that could take part couldn't be read: ${file}, so the State shown may not be the one Claude Code has.`;
      t.has(details, caveat, `la tarjeta de cd-throwaway-shape-user tiene el aviso «${caveat}»`);
      await press(s, "View", ".panel.items");
      await s.waitText(".panel.detail .panel-title", "Read failures");
      const cards = await s.exec(`
        return [...document.querySelectorAll(".panel.detail .entry-card.failure")].map((c) => ({
          tags: [...c.querySelectorAll(".tag")].map((e) => e.textContent.trim()),
          sentence: c.querySelector(".reason")?.textContent.trim(),
        }));`);
      t.check(cards.length === 1 && cards[0].tags.includes("Unexpected shape"), "el panel «Read failures» tiene una sola tarjeta, con la etiqueta «Unexpected shape»", JSON.stringify(cards));
      const sentence = `Claude Code cannot read the MCP servers at ${file}#projects.${t.root.replaceAll("\\", "/")}/project.mcpServers: it holds a list, not an object, so it reads no Entries from there.`;
      t.eq(cards[0]?.sentence, sentence, `esa tarjeta dice «${sentence}»`);
    },
  },
  {
    id: "1.4",
    async run(t) {
      const { s } = t;
      const skill = "cd-throwaway-synced:cd-throwaway-synced-skill";
      await addHome(t, "Claude Code", `${t.root}\\claude-code-synced-home`, { checks: false });
      const text = await inventoryOf(t, "Claude Code", "claude-code-synced-home");
      t.has(text, skill, `el Inventario de ese home muestra la skill «${skill}»`);
      await openItem(t, skill);
      const card = await s.exec(`
        const c = document.querySelector(".panel.detail .entry-card");
        return c && {
          tags: [...c.querySelectorAll(".tag")].map((e) => e.textContent.trim()),
          caveats: [...c.querySelectorAll(".caveat")].map((e) => e.textContent.trim()),
          notes: [...c.querySelectorAll(".action-note")].map((e) => e.textContent.trim()),
          buttons: [...c.querySelectorAll(".entry-actions button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })),
        };`);
      t.check(card?.tags.includes("cloud mirror"), "la tarjeta tiene la etiqueta «cloud mirror»", card?.tags.join(", "));
      const caveat = "Depends on Claude Code gate `tengu_claudeai_plugins_sync_enabled`: on as of 2 Oct 2026 07:40 UTC.";
      t.check(card?.caveats.includes(caveat), `la tarjeta dice «${caveat}»`, card?.caveats.join(" | "));
      const button = (name) => card?.buttons.find((b) => b.text === name);
      t.check(button("Disable")?.disabled, "el botón Disable está desactivado", JSON.stringify(card?.buttons));
      const note = "Disable is not offered: this Entry is managed, or a copy of the user's account, and config-digger only reads it.";
      t.check(card?.notes.includes(note), `bajo los botones dice «${note}»`, card?.notes.join(" | "));
      t.check(button("Remove")?.disabled, "el botón Remove está desactivado", JSON.stringify(card?.buttons));
    },
  },
];
