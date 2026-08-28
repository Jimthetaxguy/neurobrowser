//! Shared SSRF boundary for every `BrowserInterface` implementation.
//!
//! Previously this logic lived privately inside `BrowserEngine`, which meant the static
//! HTTP engine was guarded and the Tauri webview runtime — the interactive path that
//! actually drives a browser — was not. Any guard that only one implementation calls is
//! not a boundary; it is a suggestion. This module is the boundary, and both impls call
//! it.
//!
//! Design rules, each written against a specific way the previous version was bypassed:
//!
//! 1. **Fail closed.** An unparseable URL or an unresolvable host is *blocked*, never
//!    allowed. The prior code used `.ok()?` on both, so a resolution failure returned
//!    "not blocked" — a security predicate that answers "I don't know" with "yes" is
//!    worse than no predicate, because it reads as protection.
//! 2. **Canonicalize before deciding.** `::ffff:169.254.169.254` and `169.254.169.254`
//!    are the same destination. IPv4-mapped and IPv4-compatible IPv6 addresses are
//!    unwrapped to their v4 form and judged there, so one address cannot be laundered
//!    through a different spelling.
//! 3. **Judge every hop.** The pre-request URL is not the destination; a redirect is.
//!    `redirect_policy()` re-runs the same check on each hop.
//! 4. **Check every resolved address, not the first.** A hostname resolving to both a
//!    public and a private address is blocked on the private one.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

/// Why a destination was refused. Carries the offending host for the operator-facing
/// message; never carries anything derived from page content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockReason {
    /// Host resolved (or parsed) to an address in a blocked range.
    InternalAddress(String),
    /// The URL could not be parsed. Fail closed.
    Unparseable(String),
    /// The host could not be resolved. Fail closed — an attacker who can make
    /// resolution fail must not thereby win.
    Unresolvable(String),
    /// Scheme is not http/https (javascript:, data:, file:, blob:, vbscript:, ...).
    DisallowedScheme(String),
}

impl std::fmt::Display for BlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockReason::InternalAddress(host) => write!(
                f,
                "Refusing to fetch internal/loopback address (host '{host}')"
            ),
            BlockReason::Unparseable(url) => {
                write!(f, "Refusing to fetch unparseable URL ('{url}')")
            }
            BlockReason::Unresolvable(host) => write!(
                f,
                "Refusing to fetch host that could not be resolved ('{host}'); \
                 unresolvable is treated as unsafe, not as safe"
            ),
            BlockReason::DisallowedScheme(scheme) => {
                write!(f, "Refusing to navigate to disallowed scheme '{scheme}:'")
            }
        }
    }
}

/// Schemes that may never be navigated. Compared against the parsed scheme, not by
/// substring: `https://example.com/?next=javascript:x` is a legitimate URL whose *scheme*
/// is https, and a `contains("javascript:")` check both blocks it wrongly and misses
/// `JavaScript:` casing.
const DISALLOWED_SCHEMES: [&str; 5] = ["javascript", "data", "vbscript", "file", "blob"];

/// True when this address is internal and must never be reached from a page-influenced
/// navigation.
///
/// IPv6 handling canonicalizes first: an IPv4-mapped (`::ffff:a.b.c.d`) or
/// IPv4-compatible address is judged by its embedded v4 address, closing the
/// `::ffff:127.0.0.1` / `::ffff:169.254.169.254` bypass.
pub fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped().or_else(|| canonical_v4_compatible(v6)) {
                return is_blocked_v4(&v4);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || is_unique_local_v6(v6)
                || is_unicast_link_local_v6(v6)
        }
    }
}

fn is_blocked_v4(v4: &Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()   // includes 169.254.0.0/16 (cloud metadata)
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_documentation()
        || is_shared_address_space_v4(v4)  // 100.64.0.0/10 (CGNAT)
        || v4.octets()[0] == 127
}

/// 100.64.0.0/10 — carrier-grade NAT space, routable to internal infrastructure.
fn is_shared_address_space_v4(v4: &Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 100 && (64..128).contains(&o[1])
}

/// fc00::/7 — unique local addresses. `Ipv6Addr::is_unique_local` is unstable, so the
/// prefix test is written out.
fn is_unique_local_v6(v6: &Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// fe80::/10 — link-local unicast. `is_unicast_link_local` is unstable; written out.
fn is_unicast_link_local_v6(v6: &Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

/// `::a.b.c.d` (deprecated IPv4-compatible form), excluding `::` and `::1` which are
/// already handled as unspecified/loopback.
fn canonical_v4_compatible(v6: &Ipv6Addr) -> Option<Ipv4Addr> {
    let s = v6.segments();
    if s[0..6] != [0, 0, 0, 0, 0, 0] {
        return None;
    }
    let v4 = Ipv4Addr::new(
        (s[6] >> 8) as u8,
        (s[6] & 0xff) as u8,
        (s[7] >> 8) as u8,
        (s[7] & 0xff) as u8,
    );
    // `::` and `::1` are not meaningful v4-compatible addresses.
    if v4.is_unspecified() || v4 == Ipv4Addr::new(0, 0, 0, 1) {
        return None;
    }
    Some(v4)
}

/// Evaluate a URL string. `None` means allowed; `Some(reason)` means refuse.
///
/// Fails closed on every uncertainty.
pub fn blocked_reason(url: &str) -> Option<BlockReason> {
    let parsed = match url::Url::parse(url) {
        Ok(p) => p,
        Err(_) => return Some(BlockReason::Unparseable(url.to_string())),
    };

    let scheme = parsed.scheme().to_ascii_lowercase();
    if DISALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Some(BlockReason::DisallowedScheme(scheme));
    }
    if scheme != "http" && scheme != "https" {
        return Some(BlockReason::DisallowedScheme(scheme));
    }

    blocked_reason_for_parsed(&parsed)
}

/// Same check against an already-parsed URL. Used by the redirect policy, which is
/// handed a `Url` per hop.
pub fn blocked_reason_for_parsed(parsed: &url::Url) -> Option<BlockReason> {
    use url::Host;

    let port = parsed.port_or_known_default().unwrap_or(80);
    let host = match parsed.host() {
        Some(h) => h,
        None => return Some(BlockReason::Unparseable(parsed.as_str().to_string())),
    };

    match host {
        Host::Ipv4(ip) => {
            is_blocked_ip(&IpAddr::V4(ip)).then(|| BlockReason::InternalAddress(ip.to_string()))
        }
        Host::Ipv6(ip) => {
            is_blocked_ip(&IpAddr::V6(ip)).then(|| BlockReason::InternalAddress(ip.to_string()))
        }
        Host::Domain(domain) => match (domain, port).to_socket_addrs() {
            // EVERY resolved address must be safe, not merely the first.
            Ok(addrs) => {
                let mut saw_any = false;
                for addr in addrs {
                    saw_any = true;
                    if is_blocked_ip(&addr.ip()) {
                        return Some(BlockReason::InternalAddress(domain.to_string()));
                    }
                }
                if saw_any {
                    None
                } else {
                    // Resolved to nothing: unknown destination, so refuse.
                    Some(BlockReason::Unresolvable(domain.to_string()))
                }
            }
            // Fail CLOSED. The previous `.ok()?` returned "not blocked" here.
            Err(_) => Some(BlockReason::Unresolvable(domain.to_string())),
        },
    }
}

/// Redirect policy that re-validates every hop.
///
/// Without this, the guard inspects only the URL we were asked for, and a public host
/// answering `302 -> http://169.254.169.254/` is followed and its body returned as page
/// content. Attaching this to the client makes the check apply to the destination that
/// is actually fetched.
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        match blocked_reason_for_parsed(attempt.url()) {
            Some(reason) => attempt.error(reason.to_string()),
            None => attempt.follow(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact vectors two independent reviewers flagged on PR #3, plus the
    /// IPv4-mapped forms that made the guard's own named example reachable.
    #[test]
    fn blocks_ipv4_mapped_and_compatible_forms() {
        for s in [
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:10.0.0.1",
            "::ffff:192.168.1.1",
            "::7f00:1", // ::127.0.0.1 in v4-compatible form
        ] {
            let ip: IpAddr = s.parse().expect("test vector parses");
            assert!(
                is_blocked_ip(&ip),
                "{s} must be blocked (IPv4-mapped bypass)"
            );
        }
    }

    #[test]
    fn blocks_ipv6_unique_local_and_link_local() {
        for s in [
            "fd00::1",
            "fc00::1",
            "fe80::1",
            "fe80::dead:beef",
            "::1",
            "::",
        ] {
            let ip: IpAddr = s.parse().expect("test vector parses");
            assert!(is_blocked_ip(&ip), "{s} must be blocked");
        }
    }

    #[test]
    fn blocks_ipv4_internal_ranges() {
        for s in [
            "127.0.0.1",
            "169.254.169.254", // cloud metadata
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "0.0.0.0",
            "100.64.0.1", // CGNAT
        ] {
            let ip: IpAddr = s.parse().expect("test vector parses");
            assert!(is_blocked_ip(&ip), "{s} must be blocked");
        }
    }

    #[test]
    fn allows_ordinary_public_addresses() {
        for s in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
        ] {
            let ip: IpAddr = s.parse().expect("test vector parses");
            assert!(!is_blocked_ip(&ip), "{s} must NOT be blocked");
        }
    }

    #[test]
    fn fails_closed_on_unparseable_url() {
        assert!(matches!(
            blocked_reason("not a url at all"),
            Some(BlockReason::Unparseable(_))
        ));
    }

    #[test]
    fn fails_closed_on_unresolvable_host() {
        // .invalid is reserved by RFC 2606 and must never resolve.
        assert!(
            matches!(
                blocked_reason("http://nonexistent.invalid/"),
                Some(BlockReason::Unresolvable(_))
            ),
            "an unresolvable host must be refused, not allowed"
        );
    }

    #[test]
    fn blocks_literal_internal_urls() {
        for u in [
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::ffff:169.254.169.254]/",
            "http://[fd00::1]/",
            "http://192.168.0.1:8080/admin",
        ] {
            assert!(
                matches!(blocked_reason(u), Some(BlockReason::InternalAddress(_))),
                "{u} must be blocked"
            );
        }
    }

    #[test]
    fn blocks_dangerous_schemes_by_scheme_not_substring() {
        for u in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>x</script>",
            "file:///etc/passwd",
            "blob:https://example.com/uuid",
            "vbscript:msgbox",
        ] {
            assert!(
                matches!(blocked_reason(u), Some(BlockReason::DisallowedScheme(_))),
                "{u} must be blocked by scheme"
            );
        }
    }

    #[test]
    fn does_not_block_a_url_merely_mentioning_a_scheme_in_its_query() {
        // The old substring check (`contains("javascript:")`) rejected this legitimate
        // https URL. Scheme parsing does not.
        let r = blocked_reason("https://example.com/redir?next=javascript:alert(1)");
        assert!(
            !matches!(r, Some(BlockReason::DisallowedScheme(_))),
            "scheme is https; a query-string mention must not trip the scheme guard"
        );
    }
}
