import { useAppIcon } from "../lib/icons";

/** The target app's own icon, or a soft square until there is one. */
export function AppIcon({ app }: { app: string | null }) {
  const src = useAppIcon(app);
  return src ? (
    <img src={src} alt="" className="h-5 w-5 shrink-0 rounded-md" />
  ) : (
    <span className="h-5 w-5 shrink-0 rounded-md bg-surface-2" />
  );
}
