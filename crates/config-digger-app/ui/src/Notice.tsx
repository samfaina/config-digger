import { Icon, type IconName } from "./icons";

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
