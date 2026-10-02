import { Icon, type IconName } from "./icons";

export type Screen = "inventory" | "history" | "settings" | "projects" | "about";

export const SCREEN_TITLES: Record<Screen, string> = {
  inventory: "Inventory",
  history: "History · Undo",
  settings: "Settings",
  projects: "Projects & homes",
  about: "About",
};

type Props = { current: Screen; onSelect: (screen: Screen) => void };

/** The navigation rail: logo, Projects & homes, the main screens in one pill, About at the bottom. */
export function Rail({ current, onSelect }: Props) {
  const button = (screen: Screen, icon: IconName) => (
    <button
      type="button"
      className={`rail-button${current === screen ? " selected" : ""}`}
      title={SCREEN_TITLES[screen]}
      aria-label={SCREEN_TITLES[screen]}
      aria-current={current === screen ? "page" : undefined}
      onClick={() => onSelect(screen)}
    >
      <Icon name={icon} />
    </button>
  );
  return (
    <nav className="rail">
      <div className="logo" aria-label="config-digger">
        cd
      </div>
      <span className="rail-space" />
      {button("projects", "folder")}
      <div className="rail-group">
        {button("inventory", "grid")}
        {button("history", "history")}
        {button("settings", "gear")}
      </div>
      <span className="rail-space wide" />
      {button("about", "info")}
    </nav>
  );
}
