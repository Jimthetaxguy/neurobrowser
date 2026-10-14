/**
 * Address-bar classification for the desktop shell.
 *
 * A submission becomes one navigation URL. The caller passes that URL to the
 * existing navigate command, which still runs netguard. This module does not
 * fetch and does not decide whether the destination is allowed.
 */

const DUCKDUCKGO_QUERY = "https://duckduckgo.com/?q=";

/**
 * @param {string} input
 * @returns {{ kind: "navigate", url: string } | { kind: "search", url: string, query: string } | null}
 */
export function classifyOmniboxInput(input) {
  if (typeof input !== "string") return null;
  const trimmed = input.trim();
  if (!trimmed) return null;

  // A leading "?" is the force-search operator, not part of the query.
  if (trimmed.startsWith("?")) {
    const query = trimmed.slice(1).trim();
    return query ? searchFor(query) : null;
  }

  // Spaces and control characters are queries. "foo bar.com" is a search.
  if (/[\s\u0000-\u001F\u007F]/.test(trimmed)) return searchFor(trimmed);

  if (hasExplicitScheme(trimmed)) return navigateTo(trimmed);

  const url = urlForHostLike(trimmed);
  if (url) return navigateTo(url);

  return searchFor(trimmed);
}

/** "Search DuckDuckGo", "Go to", or "" when the field has nothing to submit. */
export function omniboxActionHint(input) {
  const destination = classifyOmniboxInput(input);
  if (!destination) return "";
  return destination.kind === "search" ? "Search DuckDuckGo" : "Go to";
}

function searchFor(query) {
  return {
    kind: "search",
    query,
    url: DUCKDUCKGO_QUERY + encodeURIComponent(query),
  };
}

function navigateTo(url) {
  return { kind: "navigate", url };
}

/**
 * True when the text already names a scheme.
 * `localhost:3000` and `example.com:443/a` match the scheme grammar, but the
 * part before the colon is a host, so they stay hosts. `scheme://` and any
 * other scheme are left unchanged for netguard, including javascript:, file:,
 * and ftp:.
 */
function hasExplicitScheme(input) {
  const match = /^([A-Za-z][A-Za-z0-9+.-]*):(.*)$/.exec(input);
  if (!match) return false;
  const scheme = match[1];
  const rest = match[2];
  if (rest.startsWith("//")) return true;
  if (isNavigableHost(scheme)) return false;
  return true;
}

/**
 * host[:port][/path][?query][#hash] with no scheme.
 * Localhost and IP addresses use http. Other hosts use https.
 * A bare IPv6 address is wrapped in brackets so the result is a valid URL.
 */
function urlForHostLike(input) {
  if (input.startsWith("[")) return urlForBracketedIpv6(input);

  const pathAt = input.search(/[/?#]/);
  const authority = pathAt === -1 ? input : input.slice(0, pathAt);
  const path = pathAt === -1 ? "" : input.slice(pathAt);
  if (!authority) return null;

  if (isIpv6(authority)) {
    if (path !== "") return null;
    return `http://[${authority}]`;
  }

  const colon = authority.lastIndexOf(":");
  let host = authority;
  let port = "";
  if (colon !== -1) {
    if (authority.indexOf(":") !== colon) return null;
    const portText = authority.slice(colon + 1);
    if (!isPort(portText)) return null;
    host = authority.slice(0, colon);
    port = `:${portText}`;
  }

  if (!host || !isNavigableHost(host)) return null;
  const scheme = isInsecureHost(host) ? "http" : "https";
  return `${scheme}://${host}${port}${path}`;
}

function urlForBracketedIpv6(input) {
  const end = input.indexOf("]");
  if (end < 2) return null;
  const inner = input.slice(1, end);
  if (!isIpv6(inner)) return null;
  let rest = input.slice(end + 1);
  let port = "";
  if (rest.startsWith(":")) {
    const match = /^:(\d+)(.*)$/.exec(rest);
    if (!match || !isPort(match[1])) return null;
    port = `:${match[1]}`;
    rest = match[2];
  }
  if (rest !== "" && !/^[/?#]/.test(rest)) return null;
  return `http://[${inner}]${port}${rest}`;
}

function isNavigableHost(host) {
  return isLocalhost(host) || isIpv4(host) || isDomainName(host);
}

function isInsecureHost(host) {
  return isLocalhost(host) || isIpv4(host);
}

function isLocalhost(host) {
  const bare = stripOneTrailingDot(host);
  return bare.toLowerCase() === "localhost";
}

function isIpv4(host) {
  const parts = host.split(".");
  if (parts.length !== 4) return false;
  return parts.every((part) => {
    if (!/^\d{1,3}$/.test(part)) return false;
    return Number(part) <= 255;
  });
}

function isDomainName(host) {
  if (!host || host.length > 253) return false;
  const bare = stripOneTrailingDot(host);
  if (!bare || bare.endsWith(".") || bare.includes("..")) return false;
  const labels = bare.split(".");
  if (labels.length < 2) return false;
  if (!labels.every(isDnsLabel)) return false;
  const tld = labels[labels.length - 1];
  return tld.length >= 2 && /\p{L}/u.test(tld);
}

function isDnsLabel(label) {
  if (label.length < 1 || label.length > 63) return false;
  return /^[\p{L}\p{N}](?:[\p{L}\p{N}-]{0,61}[\p{L}\p{N}])?$/u.test(label);
}

function stripOneTrailingDot(host) {
  return host.endsWith(".") ? host.slice(0, -1) : host;
}

function isPort(text) {
  if (!/^\d{1,5}$/.test(text)) return false;
  return Number(text) <= 65535;
}

/** Eight hex groups, with one `::` compression and an optional dotted IPv4 tail. */
function isIpv6(value) {
  if (!value || value.length > 45 || value.includes("%")) return false;
  const halves = value.split("::");
  if (halves.length > 2) return false;

  const parseSide = (side) => {
    if (side === "") return [];
    const pieces = side.split(":");
    const groups = [];
    for (let index = 0; index < pieces.length; index += 1) {
      const piece = pieces[index];
      const last = index === pieces.length - 1;
      if (piece.includes(".")) {
        if (!last || !isIpv4(piece)) return null;
        groups.push("v4", "v4");
        continue;
      }
      if (!/^[0-9a-fA-F]{1,4}$/.test(piece)) return null;
      groups.push(piece);
    }
    return groups;
  };

  if (halves.length === 1) {
    const groups = parseSide(halves[0]);
    return Boolean(groups && groups.length === 8);
  }
  const left = parseSide(halves[0]);
  const right = parseSide(halves[1]);
  if (!left || !right) return false;
  return left.length + right.length < 8;
}
