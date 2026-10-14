import { createElement } from "react";

/** `3 of 12`, or `No matches` when the page has none. */
export function findMatchLabel(current, total) {
  const count = Number(total) || 0;
  if (count <= 0) return "No matches";
  const shown = Number(current) > 0 ? Number(current) : 0;
  return `${shown} of ${count}`;
}

/**
 * Chrome shortcut while a modifier is held.
 * `find`, `find-next`, `find-previous`, `zoom-in`, `zoom-out`, `zoom-reset`, or null.
 * Plus is both `=` and `+`, including the numpad.
 */
export function chromeShortcut(event) {
  if (!event || event.altKey) return null;
  if (event.metaKey && event.ctrlKey) return null;
  if (!event.metaKey && !event.ctrlKey) return null;
  const key = String(event.key || "");
  const lower = key.toLowerCase();
  const code = event.code || "";
  let command = null;
  if (lower === "f" && !event.shiftKey) command = "find";
  else if (lower === "g") command = event.shiftKey ? "find-previous" : "find-next";
  else if ((key === "0" || code === "Numpad0") && !event.shiftKey) command = "zoom-reset";
  else if (!event.shiftKey && (key === "-" || code === "Minus" || code === "NumpadSubtract")) command = "zoom-out";
  else if (key === "+" || key === "=" || code === "Equal" || code === "NumpadAdd") command = "zoom-in";
  if (event.repeat && command !== "zoom-in" && command !== "zoom-out") return null;
  return command;
}

export function FindBar({ query, total, current, onQuery, onNext, onPrevious, onClose, inputRef }) {
  const label = query ? findMatchLabel(current, total) : "";
  return createElement(
    "form",
    {
      "aria-label": "Find in page",
      className: "find-bar",
      role: "search",
      onSubmit: (event) => {
        event.preventDefault();
        onNext();
      },
    },
    createElement("input", {
      "aria-label": "Find",
      ref: inputRef,
      type: "text",
      value: query,
      onInput: (event) => onQuery(event.target.value),
      onKeyDown: (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
          return;
        }
        if (event.key === "Enter") {
          event.preventDefault();
          if (event.shiftKey) onPrevious();
          else onNext();
        }
      },
    }),
    createElement(
      "span",
      { "aria-live": "polite", className: "find-count" },
      label
    ),
    createElement(
      "button",
      { "aria-label": "Previous match", onClick: onPrevious, type: "button" },
      "Previous"
    ),
    createElement(
      "button",
      { "aria-label": "Next match", onClick: onNext, type: "button" },
      "Next"
    ),
    createElement(
      "button",
      { "aria-label": "Close find", onClick: onClose, type: "button" },
      "Close"
    )
  );
}
