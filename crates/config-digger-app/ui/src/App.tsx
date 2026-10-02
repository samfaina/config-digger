import { useState } from "react";
import { InventoryScreen } from "./Inventory";
import { Rail, SCREEN_TITLES, type Screen } from "./Rail";

export function App() {
  const [screen, setScreen] = useState<Screen>("inventory");
  return (
    <div className="app">
      <Rail current={screen} onSelect={setScreen} />
      <main className="main">
        {screen === "inventory" ? <InventoryScreen /> : <ComingLater title={SCREEN_TITLES[screen]} />}
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
