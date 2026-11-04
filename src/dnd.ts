// Drag-and-drop payload types. Custom MIME types keep these drags away from
// text fields such as the terminal's input textarea.
export const PANE_MIME = "application/x-wterm-pane";
export const TAB_MIME = "application/x-wterm-tab";
export const WORKSPACE_MIME = "application/x-wterm-workspace";

export function carries(event: React.DragEvent, mime: string): boolean {
  return event.dataTransfer.types.includes(mime);
}

export function startDrag(event: React.DragEvent, mime: string, id: string): void {
  event.dataTransfer.setData(mime, id);
  event.dataTransfer.effectAllowed = "move";
}

/** True when the pointer is in the trailing half of the element along `axis`. */
export function afterMidpoint(event: React.DragEvent, axis: "x" | "y"): boolean {
  const rect = event.currentTarget.getBoundingClientRect();
  return axis === "x" ? event.clientX > rect.left + rect.width / 2 : event.clientY > rect.top + rect.height / 2;
}

/**
 * Index for a move reducer that counts positions without the moving item:
 * dropping before or after `targetIndex`, coming from `fromIndex` (-1 when the
 * item comes from another list).
 */
export function insertionIndex(fromIndex: number, targetIndex: number, after: boolean): number {
  const index = targetIndex + (after ? 1 : 0);
  return fromIndex >= 0 && fromIndex < index ? index - 1 : index;
}

export type DropZone = "left" | "right" | "top" | "bottom" | "center";

/** Edge band, as a share of the pane, where a drop splits instead of swapping. */
const EDGE = 0.28;

/** The closest edge within the band, else the center. */
export function zoneAt(x: number, y: number): DropZone {
  const distances: Array<[DropZone, number]> = [
    ["left", x],
    ["right", 1 - x],
    ["top", y],
    ["bottom", 1 - y],
  ];
  const [side, distance] = distances.reduce((best, entry) => (entry[1] < best[1] ? entry : best));
  return distance < EDGE ? side : "center";
}

export function dropZone(event: React.DragEvent): DropZone {
  const rect = event.currentTarget.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return "center";
  return zoneAt((event.clientX - rect.left) / rect.width, (event.clientY - rect.top) / rect.height);
}
