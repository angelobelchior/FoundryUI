import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent } from "react";

const interactiveSelector = [
  "button",
  "input",
  "textarea",
  "select",
  "option",
  "a",
  "[role='button']",
  "[role='menu']",
  "[role='menuitem']",
  "[contenteditable='true']",
].join(", ");

export function dragWindowFromMouseDown(event: MouseEvent<HTMLElement>) {
  if (event.button !== 0) {
    return;
  }

  const target = event.target as HTMLElement | null;

  if (target?.closest(interactiveSelector)) {
    return;
  }

  void getCurrentWindow().startDragging();
}
