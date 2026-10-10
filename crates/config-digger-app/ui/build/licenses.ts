// The third-party licenses About lists, gathered when the UI is built from
// Cargo and npm metadata: every crate the app and the CLI ship, and every npm
// package the webview bundles, the font included, each with its license text.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import type { Plugin } from "vite";

/** One component shipped in this build. */
export type ThirdPartyLicense = {
  name: string;
  version: string;
  /** Its SPDX license expression, or where to look when it has none. */
  license: string;
  source: "cargo" | "npm";
  /** What it is, when its name does not say, such as the bundled font. */
  note: string | null;
  /** The index of its license text in `texts`, `null` when it ships none. */
  text: number | null;
};

/** Every component shipped, with each license text kept once. */
export type ThirdPartyLicenses = { components: ThirdPartyLicense[]; texts: string[] };

/** A component with its license text, before the texts are shared. */
export type Component = Omit<ThirdPartyLicense, "text"> & { text: string | null };

/** What `cargo metadata --format-version 1` prints, as far as it is read here. */
export type CargoMetadata = {
  workspace_members: string[];
  packages: {
    id: string;
    name: string;
    version: string;
    license: string | null;
    license_file: string | null;
    manifest_path: string;
  }[];
  resolve: {
    nodes: { id: string; deps: { pkg: string; dep_kinds: { kind: string | null; target: string | null }[] }[] }[];
  };
};

/** A crate shipped, with the folder its sources are in. */
export type Crate = { name: string; version: string; license: string; folder: string; licenseFile: string | null };

/** What a package's `package.json` says, as far as it is read here. */
export type PackageJson = {
  name: string;
  version: string;
  license?: string | { type: string };
  licenses?: { type: string }[];
  dependencies?: Record<string, string>;
};

/** An npm package shipped, by name. */
export type NpmPackage = { name: string; version: string; license: string; note: string | null };

/** What a component without a license expression shows. */
const NO_EXPRESSION = "See its license file";

/** The platform the app ships for: only its dependencies are listed. */
const TARGET = "x86_64-pc-windows-msvc";

/** The crates the installer and the CLI zip ship. */
const SHIPPED_CRATES = ["config-digger-app", "config-digger-cli"];

/** The module About imports the licenses from. */
const MODULE = "virtual:third-party-licenses";

/**
 * The crates `roots` depend on at run time, directly or not, sorted by
 * name: neither dev nor build dependencies, nor the workspace's own crates.
 */
export function shippedCrates(metadata: CargoMetadata, roots: string[]): Crate[] {
  const own = new Set(metadata.workspace_members);
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const seen = new Set<string>();
  const queue = metadata.packages.filter((pkg) => roots.includes(pkg.name) && own.has(pkg.id)).map((pkg) => pkg.id);
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (dep.dep_kinds.some((kind) => kind.kind === null)) queue.push(dep.pkg);
    }
  }
  return [...seen]
    .filter((id) => !own.has(id))
    .map((id) => packages.get(id)!)
    .map((pkg) => ({
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? NO_EXPRESSION,
      folder: dirname(pkg.manifest_path),
      licenseFile: pkg.license_file,
    }))
    .sort(byNameAndVersion);
}

/**
 * The npm packages `dependencies` name and theirs, sorted by name, as `read`
 * finds each one's `package.json`. One not installed is left out.
 */
export function shippedPackages(
  dependencies: Record<string, string>,
  read: (name: string) => PackageJson | null,
): NpmPackage[] {
  const shipped = new Map<string, NpmPackage>();
  const queue = Object.keys(dependencies);
  while (queue.length > 0) {
    const name = queue.shift()!;
    if (shipped.has(name)) continue;
    const pkg = read(name);
    if (!pkg) continue;
    shipped.set(name, { name, version: pkg.version, license: npmLicense(pkg), note: fontNote(name) });
    queue.push(...Object.keys(pkg.dependencies ?? {}));
  }
  return [...shipped.values()].sort(byNameAndVersion);
}

/** The license expression of a package, in any shape npm has used. */
export function npmLicense(pkg: PackageJson): string {
  if (typeof pkg.license === "string") return pkg.license;
  if (pkg.license) return pkg.license.type;
  if (pkg.licenses?.length) return pkg.licenses.map((license) => license.type).join(" OR ");
  return NO_EXPRESSION;
}

/** What a Fontsource package is: the font the app bundles. */
function fontNote(name: string): string | null {
  const font = name.match(/^@fontsource\/(.+)$/)?.[1];
  return font ? `The bundled font, ${font.charAt(0).toUpperCase()}${font.slice(1)}` : null;
}

/**
 * The license and notice files among `files`, the names in a package's
 * folder, sorted: only `named` when the package names its own.
 */
export function licenseFiles(files: string[], named: string | null): string[] {
  if (named) return files.filter((file) => file === named);
  return files.filter((file) => /^(licen[cs]e|copying|notice|ofl)([-._].*)?$/i.test(file)).sort((a, b) =>
    a.toLowerCase().localeCompare(b.toLowerCase()),
  );
}

/** `components`, sorted, with each distinct license text kept once. */
export function thirdPartyLicenses(components: Component[]): ThirdPartyLicenses {
  const texts: string[] = [];
  const indexes = new Map<string, number>();
  const shared = [...components].sort(byNameAndVersion).map((component) => {
    let text: number | null = null;
    if (component.text !== null) {
      text = indexes.get(component.text) ?? texts.push(component.text) - 1;
      indexes.set(component.text, text);
    }
    return { ...component, text };
  });
  return { components: shared, texts };
}

function byNameAndVersion(a: { name: string; version: string }, b: { name: string; version: string }): number {
  return a.name.localeCompare(b.name) || a.version.localeCompare(b.version);
}

/** The license texts in `folder`, one after the other, `null` when there are none. */
function readTexts(folder: string, named: string | null): string | null {
  if (!existsSync(folder)) return null;
  const files = licenseFiles(readdirSync(folder), named);
  const texts = files.map((file) => readFileSync(join(folder, file), "utf8").trim()).filter(Boolean);
  return texts.length > 0 ? texts.join("\n\n") : null;
}

/** Reads every shipped component's license from the workspace and `node_modules` around `ui`. */
export function readThirdPartyLicenses(ui: string): ThirdPartyLicenses {
  const workspace = resolve(ui, "../../..");
  const metadata: CargoMetadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET], {
      cwd: workspace,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    }),
  );
  const crates: Component[] = shippedCrates(metadata, SHIPPED_CRATES).map((crate) => ({
    name: crate.name,
    version: crate.version,
    license: crate.license,
    source: "cargo",
    note: null,
    text: readTexts(crate.folder, crate.licenseFile),
  }));

  const modules = join(ui, "node_modules");
  const read = (name: string): PackageJson | null => {
    const file = join(modules, name, "package.json");
    return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
  };
  const own: PackageJson = JSON.parse(readFileSync(join(ui, "package.json"), "utf8"));
  const packages: Component[] = shippedPackages(own.dependencies ?? {}, read).map((pkg) => ({
    ...pkg,
    source: "npm",
    text: readTexts(join(modules, pkg.name), null),
  }));

  return thirdPartyLicenses([...crates, ...packages]);
}

/** Serves the third-party licenses as `virtual:third-party-licenses`, read once per build. */
export function thirdPartyLicensesPlugin(): Plugin {
  const resolved = `\0${MODULE}`;
  let ui = "";
  let licenses: ThirdPartyLicenses | null = null;
  return {
    name: "third-party-licenses",
    configResolved: (config) => {
      ui = config.root;
    },
    resolveId: (id) => (id === MODULE ? resolved : null),
    load(id) {
      if (id !== resolved) return null;
      licenses ??= readThirdPartyLicenses(ui);
      return `export default ${JSON.stringify(licenses)};`;
    },
  };
}
