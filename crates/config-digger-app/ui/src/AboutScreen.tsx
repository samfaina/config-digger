import { useEffect, useState } from "react";
import type { ThirdPartyLicenses } from "../build/licenses";
import { commands, type About, type Page } from "./bindings";
import { call, NOTICE_MS } from "./desktop";
import { Icon } from "./icons";
import { Status, type Notice } from "./Notice";
import { UpdateBanner } from "./UpdateBanner";
import { lastCheckedText } from "./updates";
import type { Updates } from "./useUpdates";
import { useTransient } from "./useTransient";

/** What each page About links to is called. */
const PAGE_TITLES: Record<Page, string> = {
  repository: "Repository",
  issues: "Report an issue",
  release_notes: "Release notes",
};

/** The About screen: this build, updates, privacy, licenses and links. */
export function AboutScreen({ updates }: { updates: Updates }) {
  const [about, setAbout] = useState<About | null>(null);
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [notice, setNotice, holdNotice] = useTransient<Notice>(NOTICE_MS);

  useEffect(() => {
    void call(commands.readAbout).then((result) => {
      if (result.status === "ok") setAbout(result.data);
      else setNotice({ kind: "error", text: `This build couldn't be read: ${result.error.message}` });
    });
  }, [setNotice]);

  const open = async (page: Page) => {
    const result = await call(() => commands.openPage(page));
    if (result.status === "error") setNotice({ kind: "error", text: `The browser couldn't open it: ${result.error.message}` });
  };

  const version = about?.version ?? "";
  const update = updates.status?.update ?? null;
  return (
    <div className="about">
      <header className="panel banner">
        <span className="about-logo" aria-hidden="true">
          cd
        </span>
        <div>
          <div className="caption">About</div>
          <div className="banner-title">config-digger {version}</div>
          {about && (
            <div className="caption">
              {[about.commit ? `build ${about.commit}` : "built without git", about.date, about.platform]
                .filter((part) => part !== null)
                .join(" · ")}
            </div>
          )}
        </div>
        <span className="grow" />
        <button type="button" className="cta" disabled={updates.checking} onClick={() => void updates.checkNow()}>
          <Icon name="refresh" small />
          {updates.checking ? "Checking…" : "Check now"}
        </button>
      </header>

      <div className="panes">
        <section className="panel about-main">
          <div className="scroll">
            <h3 className="place-group">
              Updates
              <span className="place-group-line" />
            </h3>
            {update && !updates.later ? (
              <UpdateBanner update={update} updates={updates} inline />
            ) : update ? (
              <div className="about-update" role="status">
                config-digger {update.version} is available. You put it off: click Check now to see it again.
              </div>
            ) : (
              updates.error && (
                <div className="about-update" role="alert">
                  {updates.error}
                </div>
              )
            )}
            {!update && updates.userChecked && !updates.error && (
              <div className="about-update" role="status">
                config-digger {version} is the latest version.
              </div>
            )}
            <p className="side-note">
              {updates.status ? lastCheckedText(updates.status.last_checked, new Date()) : "Last checked: …"} Checks
              only when you click Check now, or on startup once you turn that on in Settings.
            </p>

            <h3 className="place-group">
              Privacy
              <span className="place-group-line" />
            </h3>
            <div className="place-row">
              <span className="place-icon">
                <Icon name="lock" small />
              </span>
              <div className="place-text">
                <div className="place-name">Nothing leaves this computer</div>
                <div className="place-sub">
                  config-digger reads and writes local files only. No telemetry, no crash reports, no accounts. The one
                  network call is the update check, and only when you ask for it.
                </div>
              </div>
            </div>

            <h3 className="place-group">
              License
              <span className="place-group-line" />
            </h3>
            <div className="place-row">
              <span className="place-icon">
                <Icon name="doc" small />
              </span>
              <div className="place-text">
                <div className="place-name">MIT OR Apache-2.0</div>
                <div className="place-sub">Open source, under either license.</div>
              </div>
              <div className="place-actions">
                <button type="button" className="cta small secondary" onClick={() => setLicensesOpen(true)}>
                  Third-party licenses
                </button>
              </div>
            </div>
          </div>
        </section>

        <aside className="panel places-side">
          <h2 className="panel-title">Links</h2>
          <div className="scroll">
            {(about?.pages ?? []).map((link) => (
              <button key={link.page} type="button" className="side-row about-link" onClick={() => void open(link.page)}>
                <b>{PAGE_TITLES[link.page]}</b>
                <span className="dim mono">{link.url.replace(/^https:\/\//, "")}</span>
                <span className="side-count">
                  <Icon name="open" small />
                </span>
              </button>
            ))}
            <p className="side-note">
              The command-line tool is installed next to the app: run <span className="mono">config-digger --help</span> in a
              terminal.
            </p>
          </div>
        </aside>
      </div>

      {licensesOpen && <LicensesDialog onClose={() => setLicensesOpen(false)} />}

      <Status notice={notice} onHold={holdNotice} />
    </div>
  );
}

/** Every component this build ships, with its license and license text, read when the dialog opens. */
function LicensesDialog({ onClose }: { onClose: () => void }) {
  const [licenses, setLicenses] = useState<ThirdPartyLicenses | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    import("virtual:third-party-licenses").then(
      (module) => setLicenses(module.default),
      (thrown: unknown) => setError(String(thrown)),
    );
  }, []);

  return (
    <div className="dialog-backdrop" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <section className="dialog licenses" role="dialog" aria-modal="true" aria-labelledby="licenses-title">
        <h1 id="licenses-title" className="dialog-title">
          Third-party licenses
        </h1>
        <p className="dialog-subtitle">
          Every component shipped in this build, gathered from Cargo and npm metadata when it was built.
        </p>
        {error && (
          <div className="alert" role="alert">
            <Icon name="flag" small />
            The licenses couldn't be read: {error}
          </div>
        )}
        {licenses === null ? (
          !error && <p className="empty">Reading the licenses…</p>
        ) : (
          <div className="licenses-list">
            {licenses.components.map((component) => (
              <details key={`${component.source}:${component.name}@${component.version}`} className="license">
                <summary>
                  <b>{component.name}</b> <span className="dim">{component.version}</span>
                  {component.note && <span className="tag">{component.note}</span>}
                  <span className="grow" />
                  <span className="dim">{component.license}</span>
                </summary>
                {component.textIndex === null ? (
                  <p className="side-note">Its package ships no license text.</p>
                ) : (
                  <pre className="license-text">{licenses.texts[component.textIndex]}</pre>
                )}
              </details>
            ))}
          </div>
        )}
        <footer className="dialog-foot">
          <span className="grow" />
          <button type="button" className="cta" onClick={onClose}>
            Close
          </button>
        </footer>
      </section>
    </div>
  );
}
