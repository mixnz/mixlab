/**
 * The pure half of the Change port dialog and the Port panel.
 *
 * Which services may move is the daemon's answer (`ServiceSummary.port_movable`); this only reads
 * it. The one thing decided here is whether the field holds a number at all, so a typo is answered
 * under the field before anything is sent.
 */

/** The port typed into the field, or `null` when it is not one a client could dial. */
export function parsePort(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const port = Number(trimmed);
  return port >= 1 && port <= 65535 ? port : null;
}

/** The confirm button's label: a running service is restarted by the move, and the button says so. */
export function confirmLabel(running: boolean): "mixengine.port.change" | "mixengine.port.changeAndRestart" {
  return running ? "mixengine.port.changeAndRestart" : "mixengine.port.change";
}

/** Whether to offer a move. A daemon that did not say (`null`) is offered nothing. */
export function canChangePort(service: { port: number | null; portMovable: boolean | null }): boolean {
  return service.portMovable === true && service.port !== null;
}

/** Whether a move restarts it: the states the daemon treats as up, read the same way here so the
 *  dialog says the restart before it happens. */
export function restartsOnMove(state: string | null): boolean {
  return state === "running" || state === "starting" || state === "restarting" || state === "degraded";
}
