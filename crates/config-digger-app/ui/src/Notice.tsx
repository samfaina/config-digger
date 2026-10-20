import { Icon, type IconName } from "./icons";
import type { BusyAction } from "./busy";

/** What a notice tells the user: information, a success or an error. */
export type NoticeKind = "information" | "success" | "error";

/** A short message a screen shows for a moment, such as after a copy. */
export type Notice = { kind: NoticeKind; text: string };

const ICON: Record<NoticeKind, IconName> = { information: "info", success: "done", error: "warning" };

/** The one look of every notice, floating at the bottom of the screen. An error is read out at once. */
export function Notice({ notice }: { notice: Notice }) {
  return (
    <div className={`notice ${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>
      <Icon name={ICON[notice.kind]} small />
      <span>{notice.text}</span>
    </div>
  );
}

/** The one look of every busy indicator: a notice with a spinner and what runs. */
export function Busy({ action }: { action: BusyAction }) {
  return (
    <div className="notice busy" role="status">
      <span className="spinner" aria-hidden="true" />
      <span>{action.text}</span>
    </div>
  );
}

/**
 * The bottom of a screen: an error notice, which replaces the busy indicator;
 * else the action last started, until every action has ended; else the notice.
 */
export function Status({ notice, actions }: { notice: Notice | null; actions: BusyAction[] }) {
  const last = actions.at(-1);
  if (last && notice?.kind !== "error") return <Busy action={last} />;
  return notice ? <Notice notice={notice} /> : null;
}
