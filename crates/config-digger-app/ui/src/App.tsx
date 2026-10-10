import { useState } from "react";
import { HistoryScreen } from "./HistoryScreen";
import { InventoryScreen } from "./Inventory";
import { inventoryStart, type InventoryStart } from "./places";
import { ProjectsScreen } from "./ProjectsScreen";
import { Rail, SCREEN_TITLES, type Screen } from "./Rail";

export function App() {
  const [screen, setScreen] = useState<Screen>("inventory");
  // Where the Inventory opens when another screen opens it on something.
  const [start, setStart] = useState<InventoryStart | undefined>(undefined);
  const select = (next: Screen) => {
    setStart(undefined);
    setScreen(next);
  };
  return (
    <div className="app">
      <Rail current={screen} onSelect={select} />
      <main className="main">
        {screen === "inventory" ? (
          <InventoryScreen start={start} onOpenHistory={() => select("history")} />
        ) : screen === "history" ? (
          <HistoryScreen />
        ) : screen === "projects" ? (
          <ProjectsScreen
            onShow={(show) => {
              setStart(inventoryStart(show));
              setScreen("inventory");
            }}
            onOpenHistory={() => select("history")}
          />
        ) : (
          <ComingLater title={SCREEN_TITLES[screen]} />
        )}
      </main>
    </div>
  );
}

/** The screens later versions of the app fill in. */
function ComingLater({ title }: { title: string }) {
  return (
    <section className="panel later">
      <h1 className="panel-title">{title}</h1>
      <p className="empty">This screen is not built yet.</p>
    </section>
  );
}
