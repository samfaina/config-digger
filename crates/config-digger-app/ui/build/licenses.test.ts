import { describe, expect, it } from "vitest";
import {
  licenseFiles,
  npmLicense,
  shippedCrates,
  shippedPackages,
  thirdPartyLicenses,
  type CargoMetadata,
  type PackageJson,
} from "./licenses";

/** `cargo metadata` of a workspace whose app and CLI ship, and whose core is its own. */
function metadata(): CargoMetadata {
  const pkg = (name: string, version: string, license: string | null, extra: object = {}) => ({
    id: `${name} ${version}`,
    name,
    version,
    license,
    license_file: null,
    manifest_path: `C:\\registry\\${name}-${version}\\Cargo.toml`,
    ...extra,
  });
  const normal = [{ kind: null, target: null }];
  return {
    workspace_members: ["config-digger-app 0.1.0", "config-digger-cli 0.1.0", "config-digger-core 0.1.0"],
    packages: [
      pkg("config-digger-app", "0.1.0", "MIT OR Apache-2.0"),
      pkg("config-digger-cli", "0.1.0", "MIT OR Apache-2.0"),
      pkg("config-digger-core", "0.1.0", "MIT OR Apache-2.0"),
      pkg("serde", "1.0.0", "MIT OR Apache-2.0"),
      pkg("serde_derive", "1.0.0", "MIT OR Apache-2.0"),
      pkg("clap", "4.6.0", "MIT OR Apache-2.0"),
      pkg("tempfile", "3.27.0", "MIT OR Apache-2.0"),
      pkg("tauri-build", "2.7.0", "Apache-2.0 OR MIT"),
      pkg("ring", "0.17.0", null, { license_file: "LICENSE" }),
    ],
    resolve: {
      nodes: [
        {
          id: "config-digger-app 0.1.0",
          deps: [
            { pkg: "config-digger-core 0.1.0", dep_kinds: normal },
            { pkg: "tempfile 3.27.0", dep_kinds: [{ kind: "dev", target: null }] },
            { pkg: "tauri-build 2.7.0", dep_kinds: [{ kind: "build", target: null }] },
          ],
        },
        { id: "config-digger-cli 0.1.0", deps: [{ pkg: "clap 4.6.0", dep_kinds: normal }] },
        {
          id: "config-digger-core 0.1.0",
          deps: [
            { pkg: "serde 1.0.0", dep_kinds: normal },
            { pkg: "ring 0.17.0", dep_kinds: normal },
          ],
        },
        { id: "serde 1.0.0", deps: [{ pkg: "serde_derive 1.0.0", dep_kinds: normal }] },
        { id: "serde_derive 1.0.0", deps: [] },
        { id: "clap 4.6.0", deps: [] },
        { id: "ring 0.17.0", deps: [] },
        { id: "tempfile 3.27.0", deps: [] },
        { id: "tauri-build 2.7.0", deps: [] },
      ],
    },
  };
}

describe("shippedCrates", () => {
  it("lists the normal dependencies the app and the CLI ship, without the workspace's own crates", () => {
    const crates = shippedCrates(metadata(), ["config-digger-app", "config-digger-cli"]);

    expect(crates.map((crate) => [crate.name, crate.version, crate.license])).toEqual([
      ["clap", "4.6.0", "MIT OR Apache-2.0"],
      ["ring", "0.17.0", "See its license file"],
      ["serde", "1.0.0", "MIT OR Apache-2.0"],
      ["serde_derive", "1.0.0", "MIT OR Apache-2.0"],
    ]);
    expect(crates[0].folder).toBe("C:\\registry\\clap-4.6.0");
    expect(crates[1].licenseFile).toBe("LICENSE");
  });
});

describe("shippedPackages", () => {
  const packages: Record<string, PackageJson> = {
    react: { name: "react", version: "19.3.0", license: "MIT" },
    "react-dom": { name: "react-dom", version: "19.3.0", license: "MIT", dependencies: { scheduler: "^0.27.0" } },
    scheduler: { name: "scheduler", version: "0.27.0", license: "MIT" },
    "@fontsource/outfit": { name: "@fontsource/outfit", version: "5.3.0", license: "OFL-1.1" },
  };

  it("lists the runtime dependencies and theirs, and names the bundled font", () => {
    const shipped = shippedPackages(
      { react: "19.3.0", "react-dom": "19.3.0", "@fontsource/outfit": "5.3.0", missing: "1.0.0" },
      (name) => packages[name] ?? null,
    );

    expect(shipped.map((pkg) => [pkg.name, pkg.version, pkg.license, pkg.note])).toEqual([
      ["@fontsource/outfit", "5.3.0", "OFL-1.1", "The bundled font, Outfit"],
      ["react", "19.3.0", "MIT", null],
      ["react-dom", "19.3.0", "MIT", null],
      ["scheduler", "0.27.0", "MIT", null],
    ]);
  });
});

describe("npmLicense", () => {
  it("reads the license field in each shape npm has used", () => {
    expect(npmLicense({ name: "a", version: "1", license: "MIT" })).toBe("MIT");
    expect(npmLicense({ name: "a", version: "1", license: { type: "ISC" } })).toBe("ISC");
    expect(npmLicense({ name: "a", version: "1", licenses: [{ type: "MIT" }, { type: "Apache-2.0" }] })).toBe(
      "MIT OR Apache-2.0",
    );
    expect(npmLicense({ name: "a", version: "1" })).toBe("See its license file");
  });
});

describe("licenseFiles", () => {
  it("picks the license and notice files of a package folder, in order", () => {
    expect(
      licenseFiles(["Cargo.toml", "LICENSE-MIT", "src", "README.md", "license-apache", "COPYING", "NOTICE", "OFL.txt"], null),
    ).toEqual(["COPYING", "license-apache", "LICENSE-MIT", "NOTICE", "OFL.txt"]);
  });

  it("takes the file a crate names", () => {
    expect(licenseFiles(["LICENSE", "LICENSE-OTHER"], "LICENSE-OTHER")).toEqual(["LICENSE-OTHER"]);
  });
});

describe("thirdPartyLicenses", () => {
  it("sorts the components and keeps each license text once", () => {
    const licenses = thirdPartyLicenses([
      { name: "serde", version: "1.0.0", license: "MIT", source: "cargo", note: null, text: "MIT text" },
      { name: "react", version: "19.3.0", license: "MIT", source: "npm", note: null, text: "React MIT text" },
      { name: "clap", version: "4.6.0", license: "MIT", source: "cargo", note: null, text: "MIT text" },
      { name: "nameless", version: "1.0.0", license: "MIT", source: "npm", note: null, text: null },
    ]);

    expect(licenses.texts).toEqual(["MIT text", "React MIT text"]);
    expect(licenses.components.map((component) => [component.name, component.textIndex])).toEqual([
      ["clap", 0],
      ["nameless", null],
      ["react", 1],
      ["serde", 0],
    ]);
  });
});
