import { useState } from "react";
import type { Home, Inventory as InventoryData, Item } from "./bindings";
import { Icon } from "./icons";
import { STATE_LABELS } from "./states";
import { useInventory } from "./useInventory";

/** The Inventory screen: the top banner over the Agents, Items and detail panes. */
export function Inventory() {
  const { inventory, error, reading, refresh } = useInventory();
  const [selected, setSelected] = useState<string | null>(null);
  const selectedItem = inventory?.items.find((item) => itemKey(item) === selected) ?? null;

  return (
    <div className="inventory">
      <header className="panel banner">
        <div>
          <div className="caption">Agent</div>
          <div className="banner-title">All Agents</div>
        </div>
        <div>
          <div className="caption">Project</div>
          <div className="banner-value">No Project</div>
        </div>
        <div>
          <div className="caption">Inventory read</div>
          <div className="banner-value" title={inventory?.read_at}>
            {inventory ? readTime(inventory.read_at) : "—"}
            <span className="caption"> · on focus</span>
          </div>
        </div>
        <span className="grow" />
        <button type="button" className="cta secondary" onClick={refresh} disabled={reading}>
          <Icon name="refresh" small />
          {reading ? "Reading" : "Refresh"}
        </button>
      </header>

      <div className="panes">
        <div className="column">
          <section className="panel agents">
            <h2 className="panel-title">Agents</h2>
            <div className="tile selected">
              <Icon name="users" small />
              All Agents
            </div>
          </section>
          <section className="panel">
            <h2 className="panel-title">Entries by State</h2>
            <p className="empty">Not built yet.</p>
          </section>
          <section className="panel">
            <h2 className="panel-title">Agent homes</h2>
            <HomeList homes={inventory?.homes ?? []} />
          </section>
        </div>

        <section className="panel items">
          <h2 className="panel-title">
            Items
            <span className="grow" />
            {inventory && <span className="count">{inventory.items.length}</span>}
          </h2>
          {error && (
            <div className="alert" role="alert">
              <Icon name="flag" small />
              The inventory could not be read: {error.message}
            </div>
          )}
          {inventory && inventory.read_failures.length > 0 && (
            <div className="alert" role="status">
              <Icon name="flag" small />
              {inventory.read_failures.length === 1
                ? "1 location couldn't be read."
                : `${inventory.read_failures.length} locations couldn't be read.`}
            </div>
          )}
          <div className="scroll">
            {inventory ? (
              <ItemList inventory={inventory} selected={selected} onSelect={setSelected} />
            ) : (
              !error && <p className="empty">Reading the inventory…</p>
            )}
          </div>
        </section>

        <section className="panel detail">
          {selectedItem ? (
            <>
              <h2 className="panel-title">
                {selectedItem.name}
                <StatePill item={selectedItem} />
              </h2>
              <p className="empty">The Entries of this Item will show here.</p>
            </>
          ) : (
            <>
              <h2 className="panel-title">Details</h2>
              <p className="empty">Select an Item to see which Entry wins and why.</p>
            </>
          )}
        </section>
      </div>
    </div>
  );
}

function HomeList({ homes }: { homes: Home[] }) {
  if (homes.length === 0) return <p className="empty">No Agent home found.</p>;
  return (
    <div className="home-list">
      {homes.map((home) => (
        <div className="home-row" key={`${home.agent}|${home.home}`}>
          <span>{home.agent_name}</span>
          <span className="dim">{home.home}</span>
        </div>
      ))}
    </div>
  );
}

type ItemListProps = {
  inventory: InventoryData;
  selected: string | null;
  onSelect: (key: string) => void;
};

/** Items grouped by Agent, in the order the core sorts them. */
function ItemList({ inventory, selected, onSelect }: ItemListProps) {
  if (inventory.items.length === 0) return <p className="empty">No Items found.</p>;
  return (
    <>
      {inventory.homes.map((home) => {
        const items = inventory.items.filter(
          (item) => item.agent === home.agent && item.home === home.home,
        );
        if (items.length === 0) return null;
        const agentHomes = inventory.homes.filter((other) => other.agent === home.agent);
        return (
          <div className="item-group" key={`${home.agent}|${home.home}`}>
            <div className="group">
              {home.agent_name}
              {agentHomes.length > 1 && <span className="dim"> · {home.home}</span>}
              <span className="line" />
              {items.length}
            </div>
            {items.map((item) => (
              <ItemRow
                key={itemKey(item)}
                item={item}
                selected={itemKey(item) === selected}
                onSelect={() => onSelect(itemKey(item))}
              />
            ))}
          </div>
        );
      })}
    </>
  );
}

function ItemRow({ item, selected, onSelect }: { item: Item; selected: boolean; onSelect: () => void }) {
  // Entries come in precedence order, so the first one is the winning Entry.
  const winner = item.entries[0];
  const entries = item.entries.length === 1 ? "1 Entry" : `${item.entries.length} Entries`;
  return (
    <button type="button" className={`item-row${selected ? " selected" : ""}`} onClick={onSelect}>
      <span className="item-icon">
        <Icon name="server" small />
      </span>
      <span className="item-text">
        <span className="item-name">{item.name}</span>
        <span className="item-location" title={winner.locator}>
          {winner.scope} · {winner.locator}
        </span>
      </span>
      <span className="item-count">{entries}</span>
      <StatePill item={item} />
    </button>
  );
}

function StatePill({ item }: { item: Item }) {
  return (
    <span className={`state-pill state-${item.state}`}>
      {STATE_LABELS[item.state]}
      <span className="dot" />
    </span>
  );
}

function itemKey(item: Item): string {
  return [item.agent, item.home, item.kind, item.name].join("|");
}

/** The local time of day the inventory was read, such as `10:42`. */
function readTime(readAt: string): string {
  return new Date(readAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
