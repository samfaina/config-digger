// Section 1: the throwaway data, and the first Agent home added through the app.
import { addHome, inventoryOf, openItem } from "./common.mjs";

export default [
  {
    id: "1.1",
    async run(t) {
      const out = t.setup();
      t.has(out, `Created the throwaway homes and Projects in ${t.root}`, `el script acaba con «Created the throwaway homes and Projects in ${t.root}»`);
      for (const file of ["claude-code-home\\.claude.json", "codex-home\\config.toml", "claude-desktop-home\\claude_desktop_config.json"]) {
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
];
