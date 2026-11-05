// Which desktop Murmur runs on, for labels that differ (Cmd and Option on a
// Mac keyboard). The webview reports the platform in its user agent.
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent ?? "");
