import { useState } from "react";
import { commands, type DataFolder } from "./bindings";
import { call, NOTICE_MS } from "./desktop";
import { Icon } from "./icons";
import { Loading, ReadProgress } from "./Loading";
import { Notice } from "./Notice";
import type { InventoryStart } from "./places";
import { readCue } from "./reads";
import { dataFolderRows, parkedRows, type ParkedRow } from "./settings";
import { useSettings } from "./useSettings";
import { useTransient } from "./useTransient";

type Props = {
  /** Opens the Inventory on a parked Entry's Item. */
  onShow: (start: InventoryStart) => void;
  onOpenAbout: () => void;
};

/** The Settings screen: the update preference and the folders config-digger keeps its data in. */
export function SettingsScreen({ onShow, onOpenAbout }: Props) {
  const { read, error, reading, save } = useSettings();
  const cue = readCue({ shown: read !== null, reading, failed: error !== null });
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useTransient<Notice>(NOTICE_MS);

  const toggleUpdates = async () => {
    if (!read) return;
    setSaving(true);
    try {
      const result = await save({ check_updates_on_startup: !read.settings.check_updates_on_startup });
      if (result.status === "error") setNotice({ kind: "error", text: `The setting couldn't be saved: ${result.error.message}` });
    } finally {
      setSaving(false);
    }
  };
  const openFolder = async (folder: DataFolder) => {
    const result = await call(() => commands.openDataFolder(folder));
    if (result.status === "error") setNotice({ kind: "error", text: `It couldn't be opened: ${result.error.message}` });
    else if (!result.data) setNotice({ kind: "information", text: "config-digger has kept nothing there yet." });
  };

  const onStartup = read?.settings.check_updates_on_startup ?? false;
  return (
    <div className="settings">
      <header className="panel banner">
        <div>
          <div className="caption">Preferences</div>
          <div className="banner-title">Settings</div>
        </div>
        <span className="grow" />
        <p className="settings-about dim">
          config-digger reads by default and writes only through a previewed Operation. Nothing here changes Agent
          configuration.
        </p>
        {cue === "rereading" && <ReadProgress text="Reading the settings again" />}
      </header>

      {error && (
        <div className="alert" role="alert">
          <Icon name="flag" small />
          The settings couldn't be read: {error.message}
        </div>
      )}

      <div className="panes">
        <section className="panel settings-updates">
          <h2 className="panel-title">Updates</h2>
          <div className="setting-line">
            <div className="place-text">
              <div className="place-name">Check for updates on startup</div>
              <div className="place-sub">
                Off by default. While it is off, config-digger makes no network call unless you click Check now in About.
              </div>
            </div>
            <button
              type="button"
              role="switch"
              className={`switch${onStartup ? " on" : ""}`}
              aria-checked={onStartup}
              aria-label="Check for updates on startup"
              disabled={read === null || saving}
              onClick={() => void toggleUpdates()}
            >
              <i />
            </button>
          </div>
          <p className="side-note">
            An update never installs on its own: you get a notice with the version and its release notes, and it installs
            when you click Install and restart. The command-line tool never checks for updates.
          </p>
          <p className="side-note">
            <button type="button" className="link" onClick={onOpenAbout}>
              Version and Check now are in About →
            </button>
          </p>
        </section>

        <section className="panel settings-data">
          <h2 className="panel-title">Data on this computer</h2>
          <div className="scroll">
            {cue === "loading" ? (
              <Loading text="Reading where config-digger keeps its data…" rows={3} />
            ) : read === null ? null : (
              <>
                {dataFolderRows(read).map((row) => (
                  <div key={row.folder} className="place-row">
                    <span className="place-icon">
                      <Icon name="folder" small />
                    </span>
                    <div className="place-text">
                      <div className="place-name">{row.title}</div>
                      <div className="place-sub mono">{row.path}</div>
                      <div className="place-sub">{row.description}</div>
                      {row.folder === "parked" && <ParkedList rows={parkedRows(read.parked)} onShow={onShow} />}
                    </div>
                    <div className="place-actions">
                      <button type="button" className="cta small secondary" onClick={() => void openFolder(row.folder)}>
                        <Icon name="folder" small />
                        Open folder
                      </button>
                    </div>
                  </div>
                ))}
                <p className="side-note">
                  Uninstalling config-digger keeps all of this unless you tick "Delete config-digger data". Enable parked
                  Entries before uninstalling: the uninstaller never puts them back.
                </p>
              </>
            )}
          </div>
        </section>
      </div>

      {notice && <Notice notice={notice} />}
    </div>
  );
}

/** The parked Entries, each with a link to its Item in the Inventory. */
function ParkedList({ rows, onShow }: { rows: ParkedRow[]; onShow: (start: InventoryStart) => void }) {
  if (rows.length === 0) return <div className="parked-none">None parked.</div>;
  return (
    <ul className="parked-list">
      {rows.map((row) => (
        <li key={row.key}>
          <span className="parked-name">{row.title}</span>
          <span className="place-sub">{row.meta}</span>
          {row.start ? (
            <button type="button" className="link" onClick={() => row.start && onShow(row.start)}>
              Show in Inventory
            </button>
          ) : (
            <span className="place-sub">Unhide its Agent home in Projects &amp; homes to see it in the Inventory.</span>
          )}
        </li>
      ))}
    </ul>
  );
}
