import { Icon, type IconName } from "./icons";
import type { BusyAction } from "./busy";
import { useLeaving } from "./useLeaving";
import { useHold, type Hold } from "./useTransient";

/** What a notice tells the user: information, a success or an error. */
export type NoticeKind = "information" | "success" | "error";

/** A short message a screen shows for a moment, such as after a copy. */
export type Notice = { kind: NoticeKind; text: string };

/** How long a notice takes to leave: the length of its leaving animation in `styles.css`. */
export const LEAVE_MS = 160;

const ICON: Record<NoticeKind, IconName> = { information: "info", success: "done", error: "warning" };

/** The one look of every notice, floating at the bottom of the screen. An error is read out at once. */
export function Notice({ notice, leaving = false, onHold }: { notice: Notice; leaving?: boolean; onHold?: Hold }) {
  const hold = useHold(onHold);
  return (
    <div className={`notice ${notice.kind}${leaving ? " leaving" : ""}`} role={notice.kind === "error" ? "alert" : "status"} {...hold}>
      <Icon name={ICON[notice.kind]} small />
      <span>{notice.text}</span>
    </div>
  );
}

/** The one look of every busy indicator: a notice with a spinner and what runs. */
export function Busy({ action, leaving = false }: { action: BusyAction; leaving?: boolean }) {
  return (
    <div className={`notice busy${leaving ? " leaving" : ""}`} role="status">
      <span className="spinner" aria-hidden="true" />
      <span>{action.text}</span>
    </div>
  );
}

/**
 * The bottom of a screen: an error notice, which replaces the busy indicator;
 * else the action last started, until every action has ended; else the notice.
 * What it showed leaves with an animation once there is nothing to show.
 */
export function Status({ notice, actions = [], onHold }: { notice: Notice | null; actions?: BusyAction[]; onHold?: Hold }) {
  const last = actions.at(-1);
  const { shown, leaving, key } = useLeaving<Notice | BusyAction>(last && notice?.kind !== "error" ? last : notice, LEAVE_MS);
  if (shown === null) return null;
  if ("kind" in shown) return <Notice key={key} notice={shown} leaving={leaving} onHold={onHold} />;
  return <Busy key={key} action={shown} leaving={leaving} />;
}
