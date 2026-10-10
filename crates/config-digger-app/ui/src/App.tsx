import { useState } from "react";
import { AboutScreen } from "./AboutScreen";
import { HistoryScreen } from "./HistoryScreen";
import { InventoryScreen } from "./Inventory";
import { inventoryStart, type InventoryStart } from "./places";
import { ProjectsScreen, type Tab } from "./ProjectsScreen";
import { Rail, type Screen } from "./Rail";
import { SettingsScreen } from "./SettingsScreen";
import { UpdateNotice } from "./UpdateNotice";
import { useUpdates } from "./useUpdates";

export function App() {
  const [screen, setScreen] = useState<Screen>("inventory");
  // Where the Inventory opens when another screen opens it on something.
  const [start, setStart] = useState<InventoryStart | undefined>(undefined);
  // Where Projects & homes opens when the empty inventory opens it.
  const [placesStart, setPlacesStart] = useState<{ tab: Tab; adding: boolean } | undefined>(undefined);
  const updates = useUpdates();
  const select = (next: Screen) => {
    setStart(undefined);
    setPlacesStart(undefined);
    setScreen(next);
  };
  const showInInventory = (at: InventoryStart) => {
    setStart(at);
    setScreen("inventory");
  };
  const openHomes = (adding: boolean) => {
    setPlacesStart({ tab: "homes", adding });
    setScreen("projects");
  };
  return (
    <div className="app">
      <Rail current={screen} onSelect={select} />
      <main className="main">
        {screen === "inventory" ? (
          <InventoryScreen
            start={start}
            onOpenHistory={() => select("history")}
            onAddHome={() => openHomes(true)}
            onOpenHomes={() => openHomes(false)}
          />
        ) : screen === "history" ? (
          <HistoryScreen />
        ) : screen === "projects" ? (
          <ProjectsScreen
            start={placesStart}
            onShow={(show) => showInInventory(inventoryStart(show))}
            onOpenHistory={() => select("history")}
          />
        ) : screen === "settings" ? (
          <SettingsScreen onShow={showInInventory} onOpenAbout={() => select("about")} />
        ) : (
          <AboutScreen updates={updates} />
        )}
      </main>
      {updates.status?.update && !updates.later && screen !== "about" && (
        <UpdateNotice update={updates.status.update} updates={updates} />
      )}
    </div>
  );
}
