function neurobrowserFind(query, index) {
  const clear = () => {
    if (window.CSS && CSS.highlights) {
      CSS.highlights.delete("neurobrowser-find");
      CSS.highlights.delete("neurobrowser-find-current");
    }
    const selection = window.getSelection && window.getSelection();
    if (selection) selection.removeAllRanges();
    return { total: 0, current: 0 };
  };
  const needle = String(query || "");
  if (!needle) return clear();
  const root = document.body || document.documentElement;
  if (!root) return clear();
  if (!document.getElementById("neurobrowser-find-style")) {
    const style = document.createElement("style");
    style.id = "neurobrowser-find-style";
    style.textContent = "::highlight(neurobrowser-find){background-color:#fff3a3;color:#111}::highlight(neurobrowser-find-current){background-color:#f5a623;color:#111}";
    (document.head || document.documentElement).appendChild(style);
  }
  const folded = needle.toLowerCase();
  const matches = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  while (node && matches.length < 2000) {
    const parent = node.parentElement;
    const tag = parent && parent.tagName;
    if (tag !== "SCRIPT" && tag !== "STYLE" && tag !== "NOSCRIPT") {
      const hay = String(node.textContent || "").toLowerCase();
      let from = 0;
      while (from < hay.length && matches.length < 2000) {
        const at = hay.indexOf(folded, from);
        if (at < 0) break;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + folded.length);
        matches.push(range);
        from = at + folded.length;
      }
    }
    node = walker.nextNode();
  }
  const total = matches.length;
  if (!total) return clear();
  const currentIndex = ((Number(index) % total) + total) % total;
  if (window.CSS && CSS.highlights && typeof Highlight === "function") {
    CSS.highlights.set("neurobrowser-find", new Highlight(...matches));
    CSS.highlights.set("neurobrowser-find-current", new Highlight(matches[currentIndex]));
  }
  const selection = window.getSelection && window.getSelection();
  if (selection) {
    selection.removeAllRanges();
    selection.addRange(matches[currentIndex]);
  }
  const currentNode = matches[currentIndex].startContainer;
  const element = currentNode && currentNode.nodeType === 1 ? currentNode : currentNode && currentNode.parentElement;
  if (element && element.scrollIntoView) element.scrollIntoView({ block: "center", inline: "nearest" });
  return { total, current: currentIndex + 1 };
}
