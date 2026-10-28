// App icons saved by the backend, fetched once per app for the window's life.

import { useEffect, useState } from "react";
import { api } from "./api";

const cache = new Map<string, Promise<string | null>>();

export function appIcon(app: string): Promise<string | null> {
  let pending = cache.get(app);
  if (!pending) {
    pending = api.appIcon(app).catch(() => null);
    cache.set(app, pending);
  }
  return pending;
}

/** Data URL of the app's icon, or null while loading or when there is none. */
export function useAppIcon(app: string | null): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (!app) return;
    let cancelled = false;
    void appIcon(app).then((url) => !cancelled && setSrc(url));
    return () => {
      cancelled = true;
    };
  }, [app]);
  return src;
}
