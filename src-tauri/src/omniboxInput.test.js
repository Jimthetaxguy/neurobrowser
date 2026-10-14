import assert from "node:assert/strict";
import { test } from "node:test";
import { classifyOmniboxInput, omniboxActionHint } from "./omniboxInput.js";

function navigate(url) {
  return { kind: "navigate", url };
}

function search(query) {
  return {
    kind: "search",
    query,
    url: `https://duckduckgo.com/?q=${encodeURIComponent(query)}`,
  };
}

test("blank address text and a bare question mark do not navigate", () => {
  for (const input of ["", "   ", "\n\t", "?", " ? ", "?\t", null, undefined, 12, {}]) {
    assert.equal(classifyOmniboxInput(input), null, `input ${JSON.stringify(input)}`);
    assert.equal(omniboxActionHint(input), "");
  }
});

test("leading and trailing spaces are ignored and internal spaces stay a search", () => {
  assert.deepEqual(classifyOmniboxInput("  example.com/docs  "), navigate("https://example.com/docs"));
  assert.deepEqual(classifyOmniboxInput("\texample.com\t"), navigate("https://example.com"));
  assert.deepEqual(classifyOmniboxInput("  https://example.com/a  "), navigate("https://example.com/a"));
  assert.deepEqual(classifyOmniboxInput("foo bar.com"), search("foo bar.com"));
  assert.deepEqual(classifyOmniboxInput("  foo  bar.com  "), search("foo  bar.com"));
  assert.deepEqual(classifyOmniboxInput("foo\tbar.com"), search("foo\tbar.com"));
  assert.deepEqual(classifyOmniboxInput("example.com/foo bar"), search("example.com/foo bar"));
  assert.equal(classifyOmniboxInput("foo bar.com").url.includes("+"), false);
  assert.match(classifyOmniboxInput("foo bar.com").url, /q=foo%20bar\.com$/);
});

test("a leading question mark forces a search even when the rest is a URL", () => {
  assert.deepEqual(classifyOmniboxInput("?example.com"), search("example.com"));
  assert.deepEqual(classifyOmniboxInput("?localhost"), search("localhost"));
  assert.deepEqual(classifyOmniboxInput("?https://example.com"), search("https://example.com"));
  assert.deepEqual(classifyOmniboxInput("  ? cats & dogs  "), search("cats & dogs"));
  assert.deepEqual(classifyOmniboxInput("??cats"), search("?cats"));
  assert.equal(omniboxActionHint("?example.com"), "Search DuckDuckGo");
});

test("single words without a dot are searches, and localhost is a page", () => {
  for (const word of ["foo", "neurobrowser", "com", "intranet", "http"]) {
    assert.deepEqual(classifyOmniboxInput(word), search(word), word);
    assert.equal(omniboxActionHint(word), "Search DuckDuckGo");
  }
  assert.deepEqual(classifyOmniboxInput("localhost"), navigate("http://localhost"));
  assert.deepEqual(classifyOmniboxInput("LOCALHOST"), navigate("http://LOCALHOST"));
  assert.deepEqual(classifyOmniboxInput("localhost:3000"), navigate("http://localhost:3000"));
  assert.deepEqual(classifyOmniboxInput("localhost:3000/app"), navigate("http://localhost:3000/app"));
  assert.deepEqual(classifyOmniboxInput("localhost/x?y=1#z"), navigate("http://localhost/x?y=1#z"));
  assert.deepEqual(classifyOmniboxInput("localhost."), navigate("http://localhost."));
  assert.equal(omniboxActionHint("localhost"), "Go to");
});

test("hostnames gain https and keep port, path, query, and fragment", () => {
  const cases = [
    ["example.com", "https://example.com"],
    ["Example.COM", "https://Example.COM"],
    ["www.example.com", "https://www.example.com"],
    ["example.co.uk", "https://example.co.uk"],
    ["my-site.example", "https://my-site.example"],
    ["a.bc", "https://a.bc"],
    ["example.com.", "https://example.com."],
    ["example.com/a/b", "https://example.com/a/b"],
    ["example.com:8443", "https://example.com:8443"],
    ["example.com:8443/a", "https://example.com:8443/a"],
    ["example.com:443/a?b=1#c", "https://example.com:443/a?b=1#c"],
    ["example.com?q=1", "https://example.com?q=1"],
    ["example.com#frag", "https://example.com#frag"],
    ["foo.localhost", "https://foo.localhost"],
    ["münchen.de", "https://münchen.de"],
    ["xn--n3h.com", "https://xn--n3h.com"],
    ["example.com/café", "https://example.com/café"],
  ];
  for (const [input, url] of cases) {
    assert.deepEqual(classifyOmniboxInput(input), navigate(url), input);
    assert.equal(omniboxActionHint(input), "Go to");
  }
});

test("IP addresses use http, including public addresses, ports, and IPv6", () => {
  const cases = [
    ["127.0.0.1", "http://127.0.0.1"],
    ["127.0.0.1:8080", "http://127.0.0.1:8080"],
    ["127.0.0.1:8080/foo", "http://127.0.0.1:8080/foo"],
    ["127.0.0.1/secret", "http://127.0.0.1/secret"],
    ["8.8.8.8", "http://8.8.8.8"],
    ["8.8.8.8:53", "http://8.8.8.8:53"],
    ["0.0.0.0", "http://0.0.0.0"],
    ["255.255.255.255", "http://255.255.255.255"],
    ["::1", "http://[::1]"],
    ["2001:db8::1", "http://[2001:db8::1]"],
    ["::ffff:127.0.0.1", "http://[::ffff:127.0.0.1]"],
    ["[::1]", "http://[::1]"],
    ["[::1]:8080", "http://[::1]:8080"],
    ["[::1]:8080/a?b=1#c", "http://[::1]:8080/a?b=1#c"],
    ["[2001:db8::1]/path", "http://[2001:db8::1]/path"],
  ];
  for (const [input, url] of cases) {
    assert.deepEqual(classifyOmniboxInput(input), navigate(url), input);
  }
});

test("an explicit scheme is unchanged so netguard still sees it", () => {
  const urls = [
    "https://example.com/path?q=1#h",
    "http://example.com",
    "HTTP://Example.COM/Path",
    "http://127.0.0.1/",
    "http://localhost:3000",
    "https://127.0.0.1",
    "https://localhost",
    "https://8.8.8.8",
    "ftp://files.example/a",
    "javascript:alert(1)",
    "JavaScript:alert(1)",
    "data:text/html,hi",
    "file:///etc/passwd",
    "blob:https://example.com/uuid",
    "vbscript:msgbox(1)",
    "about:blank",
    "mailto:user@example.com",
    "user:pass@example.com",
    "http://",
    "https://user:pass@example.com",
  ];
  for (const url of urls) {
    assert.deepEqual(classifyOmniboxInput(url), navigate(url), url);
    assert.deepEqual(classifyOmniboxInput(`  ${url}  `), navigate(url), url);
  }
});

test("text that only looks a bit like a URL is a DuckDuckGo search", () => {
  const queries = [
    "a.b",
    "foo.c",
    ".com",
    "example.",
    "example..com",
    "-example.com",
    "example-.com",
    "foo_bar.com",
    "not a url",
    "alice@example.com",
    "999.999.999.999",
    "1.2.3",
    "127.1",
    "example.123",
    "example.com:65536",
    "example.com:99999",
    "example.com:",
    "localhost:",
    "[::1]:",
    "[::1]:99999",
    "[not-ipv6]",
    "::1/path",
    "1:2:3:4:5:6:7",
    "///example.com",
    "example.com:443abc",
  ];
  for (const query of queries) {
    const result = classifyOmniboxInput(query);
    assert.deepEqual(result, search(query), query);
    assert.match(result.url, /^https:\/\/duckduckgo\.com\/\?q=/);
  }
});

test("search queries are encoded and special characters cannot change the host", () => {
  assert.deepEqual(classifyOmniboxInput("cats & dogs"), search("cats & dogs"));
  assert.deepEqual(classifyOmniboxInput("C++"), search("C++"));
  assert.equal(classifyOmniboxInput("100%").url, "https://duckduckgo.com/?q=100%25");
  assert.equal(
    classifyOmniboxInput("café menu").url,
    `https://duckduckgo.com/?q=${encodeURIComponent("café menu")}`
  );
  const spacedScheme = classifyOmniboxInput("javascript: alert(1)");
  assert.equal(spacedScheme.kind, "search");
  assert.equal(spacedScheme.url.startsWith("https://duckduckgo.com/?q="), true);
  assert.equal(spacedScheme.url.includes("javascript:"), false);
  const nul = classifyOmniboxInput("example.com\u0000.evil");
  assert.equal(nul.kind, "search");
  assert.match(nul.url, /^https:\/\/duckduckgo\.com\/\?q=/);
});

test("the hint names a search or a navigation and is empty when there is nothing to open", () => {
  assert.equal(omniboxActionHint("cats"), "Search DuckDuckGo");
  assert.equal(omniboxActionHint("  example.com/a  "), "Go to");
  assert.equal(omniboxActionHint("https://example.com"), "Go to");
  assert.equal(omniboxActionHint(""), "");
});
