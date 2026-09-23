// A background tab title update must not replace an in-progress URL edit.
export function nativePageUpdates(event, activePageId) {
  if (event.type === "tabs") {
    const updates = { tabs: event.tabs, activePageId: event.activePageId };
    if (event.activePageId !== activePageId) {
      updates.url = event.tabs.find((tab) => tab.id === event.activePageId)?.url || "";
    }
    return updates;
  }
  if (event.type === "snapshot" && event.pageId === activePageId) {
    return { snapshot: event.snapshot };
  }
  return {};
}
