/**
 * Saved connections that still point at a MixEngine service's old port, once it has moved.
 *
 * Only a connection MixEngine made carries the service it is for, in its `keyringRef`
 * (`<service-id>/<user>`); one typed by hand could mean anything on that port, and is left alone.
 */
import type { SavedConnection } from "../types";

/** The connections made for `service` that are still on `from`. */
export function connectionsToFollow(saved: SavedConnection[], service: string, from: number): SavedConnection[] {
  // The slash is part of the match: `mysql@8` must not take `mysql@80`'s connections.
  const prefix = `${service}/`;
  return saved.filter((entry) => entry.keyringRef?.startsWith(prefix) === true && entry.config.port === from);
}

/** `entry`, moved to `to`. A copy: the list it came from is the store's. */
export function followed(entry: SavedConnection, to: number): SavedConnection {
  return { ...entry, config: { ...entry.config, port: to } };
}
