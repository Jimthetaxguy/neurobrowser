//! Whether a page may be stored.
//!
//! [`CapturePolicy`] follows the domain half of `neurobrowser`'s `ActionPolicy`:
//! a denylist, an optional allowlist, and [`CapturePolicy::evaluate`]. An empty
//! allowlist allows every host that is not denied. Evaluation fails closed when
//! capture is disabled, the URL cannot be parsed, the scheme cannot be governed
//! by a host rule, or the URL has no host.

use serde::{Deserialize, Serialize};

/// Schemes that carry no page host the domain lists can govern, or that read
/// local files. Same set `ActionPolicy` refuses for navigation.
const UNGOVERNABLE_SCHEMES: [&str; 5] = ["javascript", "data", "vbscript", "file", "blob"];

/// Gate for [`crate::MemoryService::capture`].
///
/// Fields are public so [`crate::MemoryService::forget`] can tombstone a host
/// by pushing it onto [`Self::denied_domains`]. The caller owns this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturePolicy {
    /// Master switch. When `false`, every URL is denied.
    pub enabled: bool,
    /// Hosts that may be captured. Empty means every host that is not denied.
    pub allowed_domains: Vec<String>,
    /// Hosts that must not be captured. A match here wins over [`Self::allowed_domains`].
    pub denied_domains: Vec<String>,
}

impl Default for CapturePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: Vec::new(),
            denied_domains: Vec::new(),
        }
    }
}

/// Result of [`CapturePolicy::evaluate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureDecision {
    Allow,
    Deny { reason: String },
}

impl CapturePolicy {
    /// Decide whether the page at `url` may be stored.
    ///
    /// `url` is a string so an unparseable value is denied instead of rejected
    /// by the type. Callers that already hold a [`url::Url`] pass `as_str()`.
    pub fn evaluate(&self, url: &str) -> CaptureDecision {
        if !self.enabled {
            return CaptureDecision::Deny {
                reason: "Capture is disabled".to_string(),
            };
        }

        let Some(parsed) = url::Url::parse(url).ok() else {
            return CaptureDecision::Deny {
                reason: "URL could not be parsed".to_string(),
            };
        };

        if let Some(scheme) = ungovernable_scheme(&parsed) {
            return CaptureDecision::Deny {
                reason: format!("URL scheme '{scheme}' cannot be captured"),
            };
        }

        let Some(domain) = parsed.host_str().map(|host| host.to_lowercase()) else {
            return CaptureDecision::Deny {
                reason: "URL has no host to evaluate".to_string(),
            };
        };

        if self
            .denied_domains
            .iter()
            .any(|denied| domain_matches(&domain, denied))
        {
            return CaptureDecision::Deny {
                reason: format!("Domain '{domain}' is denied by policy"),
            };
        }

        if !self.allowed_domains.is_empty()
            && !self
                .allowed_domains
                .iter()
                .any(|allowed| domain_matches(&domain, allowed))
        {
            return CaptureDecision::Deny {
                reason: format!("Domain '{domain}' is not in the allowlist"),
            };
        }

        CaptureDecision::Allow
    }
}

fn ungovernable_scheme(url: &url::Url) -> Option<String> {
    let scheme = url.scheme().to_ascii_lowercase();
    UNGOVERNABLE_SCHEMES
        .contains(&scheme.as_str())
        .then_some(scheme)
}

/// `rule` matches `domain` exactly or as a parent of it.
///
/// The rule is trimmed, a leading dot is stripped, and the comparison is
/// case-insensitive. `notexample.com` does not match `example.com`.
fn domain_matches(domain: &str, rule: &str) -> bool {
    let rule = rule.trim().trim_start_matches('.').to_lowercase();
    domain == rule || domain.ends_with(&format!(".{rule}"))
}

#[cfg(test)]
mod tests {
    use super::{CaptureDecision, CapturePolicy};

    fn policy(enabled: bool, allowed: &[&str], denied: &[&str]) -> CapturePolicy {
        CapturePolicy {
            enabled,
            allowed_domains: allowed.iter().map(|domain| (*domain).to_string()).collect(),
            denied_domains: denied.iter().map(|domain| (*domain).to_string()).collect(),
        }
    }

    fn deny(reason: &str) -> CaptureDecision {
        CaptureDecision::Deny {
            reason: reason.to_string(),
        }
    }

    #[test]
    fn default_is_enabled_with_empty_domain_lists() {
        let policy = CapturePolicy::default();
        assert!(policy.enabled);
        assert!(policy.allowed_domains.is_empty());
        assert!(policy.denied_domains.is_empty());
        assert_eq!(
            policy.evaluate("https://example.com/docs"),
            CaptureDecision::Allow
        );
    }

    #[test]
    fn evaluate_applies_domain_rules() {
        struct Case {
            name: &'static str,
            policy: CapturePolicy,
            url: &'static str,
            expected: CaptureDecision,
        }

        let cases = [
            Case {
                name: "disabled denies an otherwise allowed host",
                policy: policy(false, &["example.com"], &[]),
                url: "https://example.com/docs",
                expected: deny("Capture is disabled"),
            },
            Case {
                name: "disabled denies before parse",
                policy: policy(false, &[], &[]),
                url: "not a url",
                expected: deny("Capture is disabled"),
            },
            Case {
                name: "denylist beats allowlist",
                policy: policy(true, &["example.com"], &["blocked.example.com"]),
                url: "https://blocked.example.com/path",
                expected: deny("Domain 'blocked.example.com' is denied by policy"),
            },
            Case {
                name: "denied parent covers subdomain",
                policy: policy(true, &[], &["example.com"]),
                url: "https://a.b.example.com/x",
                expected: deny("Domain 'a.b.example.com' is denied by policy"),
            },
            Case {
                name: "allowlist blocks outsiders",
                policy: policy(true, &["example.com"], &[]),
                url: "https://other.test/page",
                expected: deny("Domain 'other.test' is not in the allowlist"),
            },
            Case {
                name: "allowlist permits the host",
                policy: policy(true, &["example.com"], &[]),
                url: "https://example.com/a",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "allowlist permits a subdomain",
                policy: policy(true, &["example.com"], &[]),
                url: "https://docs.example.com/guide",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "parent does not match a more specific rule",
                policy: policy(true, &["docs.example.com"], &[]),
                url: "https://example.com/",
                expected: deny("Domain 'example.com' is not in the allowlist"),
            },
            Case {
                name: "suffix without a dot boundary does not match",
                policy: policy(true, &["example.com"], &[]),
                url: "https://notexample.com/",
                expected: deny("Domain 'notexample.com' is not in the allowlist"),
            },
            Case {
                name: "embedded rule text is not a parent match",
                policy: policy(true, &[], &["example.com"]),
                url: "https://example.com.evil.com/",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "host and rule are case insensitive",
                policy: policy(true, &["Example.COM"], &[]),
                url: "https://Docs.Example.com/x",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "leading dot and surrounding space are ignored on the rule",
                policy: policy(true, &["  .example.com  "], &[]),
                url: "https://example.com/",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "port and userinfo are not part of the host",
                policy: policy(true, &[], &["example.com"]),
                url: "https://user:secret@example.com:8443/a",
                expected: deny("Domain 'example.com' is denied by policy"),
            },
            Case {
                name: "empty allowlist allows an ordinary host",
                policy: policy(true, &[], &[]),
                url: "https://anywhere.test/item",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "loopback is a domain decision",
                policy: policy(true, &[], &[]),
                url: "http://127.0.0.1/admin",
                expected: CaptureDecision::Allow,
            },
            Case {
                name: "denied loopback is still denied",
                policy: policy(true, &[], &["127.0.0.1"]),
                url: "http://127.0.0.1/admin",
                expected: deny("Domain '127.0.0.1' is denied by policy"),
            },
            Case {
                name: "unparseable url is denied",
                policy: policy(true, &[], &[]),
                url: "not a url",
                expected: deny("URL could not be parsed"),
            },
            Case {
                name: "empty host does not parse and is denied",
                policy: policy(true, &[], &[]),
                url: "https://",
                expected: deny("URL could not be parsed"),
            },
            Case {
                name: "javascript scheme is denied",
                policy: policy(true, &["example.com"], &[]),
                url: "javascript:alert(1)",
                expected: deny("URL scheme 'javascript' cannot be captured"),
            },
            Case {
                name: "data scheme is denied",
                policy: policy(true, &[], &[]),
                url: "data:text/plain,hi",
                expected: deny("URL scheme 'data' cannot be captured"),
            },
            Case {
                name: "file scheme is denied even when the host is allowlisted",
                policy: policy(true, &["example.com"], &[]),
                url: "file://example.com/tmp/secret",
                expected: deny("URL scheme 'file' cannot be captured"),
            },
            Case {
                name: "blob scheme is denied",
                policy: policy(true, &[], &[]),
                url: "blob:https://example.com/uuid",
                expected: deny("URL scheme 'blob' cannot be captured"),
            },
            Case {
                name: "vbscript scheme is denied",
                policy: policy(true, &[], &[]),
                url: "vbscript:msgbox(1)",
                expected: deny("URL scheme 'vbscript' cannot be captured"),
            },
            Case {
                name: "hostless url is denied",
                policy: policy(true, &[], &[]),
                url: "about:blank",
                expected: deny("URL has no host to evaluate"),
            },
        ];

        for case in cases {
            assert_eq!(
                case.policy.evaluate(case.url),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
