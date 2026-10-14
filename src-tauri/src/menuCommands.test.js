import assert from "node:assert/strict";
import { test } from "node:test";
import {
  MENU_COMMAND_EVENT,
  MENU_ECHO_MS,
  bindMenuCommand,
  createMenuEchoGuard,
  menuCommandFromEvent,
  numberedTabCommand,
} from "./menuCommands.js";

test("a menu event payload is the command string", () => {
  assert.equal(menuCommandFromEvent({ payload: "reload" }), "reload");
  assert.equal(menuCommandFromEvent({ payload: { command: "back" } }), "back");
  assert.equal(menuCommandFromEvent("focus-address"), "focus-address");
  assert.equal(menuCommandFromEvent({ payload: "" }), null);
  assert.equal(menuCommandFromEvent({ payload: { command: "" } }), null);
  assert.equal(menuCommandFromEvent(null), null);
  assert.equal(menuCommandFromEvent({}), null);
});

test("numbered tab commands are 1 through 9", () => {
  assert.equal(numberedTabCommand("select-tab-1"), 1);
  assert.equal(numberedTabCommand("select-tab-9"), 9);
  assert.equal(numberedTabCommand("select-tab-0"), null);
  assert.equal(numberedTabCommand("select-tab-10"), null);
  assert.equal(numberedTabCommand("new-tab"), null);
  assert.equal(numberedTabCommand(null), null);
});

test("a second delivery inside the echo window is ignored, and a later one is not", () => {
  let clock = 1_000;
  const guard = createMenuEchoGuard(() => clock);
  guard.mark("new-tab");
  assert.equal(guard.isEcho("new-tab"), true);
  clock += MENU_ECHO_MS - 1;
  assert.equal(guard.isEcho("new-tab"), true);
  clock += 1;
  assert.equal(guard.isEcho("new-tab"), false);
  assert.equal(guard.isEcho("close-tab"), false);
  guard.mark("back", 50);
  assert.equal(guard.isEcho("back", 50), true);
  assert.equal(guard.isEcho("back", 50 + MENU_ECHO_MS), false);
});

test("menu listen forwards the command and unsubscribe stops it", async () => {
  const seen = [];
  let handler = null;
  const stop = bindMenuCommand((name, next) => {
    assert.equal(name, MENU_COMMAND_EVENT);
    handler = next;
    return () => {
      handler = null;
    };
  }, (command) => {
    seen.push(command);
  });
  await Promise.resolve();
  await Promise.resolve();
  handler({ payload: "next-tab" });
  handler({ payload: { command: "previous-tab" } });
  handler({ payload: "" });
  assert.deepEqual(seen, ["next-tab", "previous-tab"]);
  stop();
  assert.equal(handler, null);
});
