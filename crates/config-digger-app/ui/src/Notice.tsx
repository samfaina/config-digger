import { Icon, type IconName } from "./icons";
import type { Running } from "./running";

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
export function Busy({ running }: { running: Running }) {
  return (
    <div className="notice busy" role="status">
      <span className="spinner" aria-hidden="true" />
      <span>{running.text}</span>
    </div>
  );
}

/** The bottom of a screen: its notice, which replaces the busy indicator, or else the action last started. */
export function Status({ notice, running }: { notice: Notice | null; running: Running[] }) {
  if (notice) return <Notice notice={notice} />;
  const last = running.at(-1);
  return last ? <Busy running={last} /> : null;
}
