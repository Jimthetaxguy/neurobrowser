//! Find-in-page cursor for one shell process.
//!
//! The page webview runs the search. This module only remembers which match
//! the bar last showed, so Next and Previous stay on that page.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const PAGE_FIND_FUNCTION: &str = include_str!("pageFindFunction.js");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindDirection {
    Start,
    Next,
    Previous,
}

impl FindDirection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "start" => Some(Self::Start),
            "next" => Some(Self::Next),
            "previous" => Some(Self::Previous),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindHit {
    pub total: usize,
    pub current: usize,
}

struct Cursor {
    query: String,
    index: usize,
    total: usize,
}

pub struct PageFind {
    cursors: Mutex<HashMap<usize, Cursor>>,
}

impl Default for PageFind {
    fn default() -> Self {
        Self {
            cursors: Mutex::new(HashMap::new()),
        }
    }
}

impl PageFind {
    /// Zero-based match to ask the page for. A new query, or a page that has
    /// not been searched yet, starts at the first match.
    pub fn index_for(&self, page_id: usize, query: &str, direction: FindDirection) -> usize {
        if query.is_empty() || direction == FindDirection::Start {
            return 0;
        }
        let cursors = self.lock();
        let Some(cursor) = cursors.get(&page_id) else {
            return 0;
        };
        if cursor.query != query || cursor.total == 0 {
            return 0;
        }
        match direction {
            FindDirection::Next => (cursor.index + 1) % cursor.total,
            FindDirection::Previous => (cursor.index + cursor.total - 1) % cursor.total,
            FindDirection::Start => 0,
        }
    }

    pub fn note(&self, page_id: usize, query: &str, hit: &FindHit) {
        let mut cursors = self.lock();
        if query.is_empty() || hit.total == 0 || hit.current == 0 {
            cursors.remove(&page_id);
            return;
        }
        cursors.insert(
            page_id,
            Cursor {
                query: query.to_string(),
                index: hit.current.saturating_sub(1).min(hit.total - 1),
                total: hit.total,
            },
        );
    }

    pub fn clear(&self, page_id: usize) {
        self.lock().remove(&page_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<usize, Cursor>> {
        self.cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// Expression the page webview evaluates. `query` is JSON text, so a quote
/// or a newline in the search cannot close the script.
pub fn search_expression(query: &str, index: usize) -> String {
    let query_json = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".to_string());
    format!("(() => {{ {PAGE_FIND_FUNCTION}\nreturn neurobrowserFind({query_json}, {index}); }})()")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_previous_wrap_the_matches_for_one_query() {
        let find = PageFind::default();
        assert_eq!(find.index_for(1, "cat", FindDirection::Start), 0);
        find.note(
            1,
            "cat",
            &FindHit {
                total: 3,
                current: 1,
            },
        );
        assert_eq!(find.index_for(1, "cat", FindDirection::Next), 1);
        find.note(
            1,
            "cat",
            &FindHit {
                total: 3,
                current: 2,
            },
        );
        assert_eq!(find.index_for(1, "cat", FindDirection::Next), 2);
        find.note(
            1,
            "cat",
            &FindHit {
                total: 3,
                current: 3,
            },
        );
        assert_eq!(find.index_for(1, "cat", FindDirection::Next), 0);
        find.note(
            1,
            "cat",
            &FindHit {
                total: 3,
                current: 1,
            },
        );
        assert_eq!(find.index_for(1, "cat", FindDirection::Previous), 2);
    }

    #[test]
    fn a_new_query_starts_over_and_an_empty_query_forgets_the_page() {
        let find = PageFind::default();
        find.note(
            4,
            "cat",
            &FindHit {
                total: 2,
                current: 2,
            },
        );
        assert_eq!(find.index_for(4, "dog", FindDirection::Next), 0);
        find.note(
            4,
            "",
            &FindHit {
                total: 0,
                current: 0,
            },
        );
        assert_eq!(find.index_for(4, "cat", FindDirection::Next), 0);
        find.note(
            4,
            "cat",
            &FindHit {
                total: 2,
                current: 1,
            },
        );
        find.clear(4);
        assert_eq!(find.index_for(4, "cat", FindDirection::Next), 0);
    }

    #[test]
    fn the_search_expression_keeps_a_quote_inside_the_query_string() {
        let expression = search_expression("a\");alert(1)", 0);
        assert!(expression.contains("neurobrowserFind("));
        assert!(expression.contains(r#"a\");alert(1)"#));
        assert!(!expression.contains("neurobrowserFind(\"a\");alert(1)\""));
        assert_eq!(FindDirection::parse("start"), Some(FindDirection::Start));
        assert_eq!(FindDirection::parse("next"), Some(FindDirection::Next));
        assert_eq!(
            FindDirection::parse("previous"),
            Some(FindDirection::Previous)
        );
        assert_eq!(FindDirection::parse("up"), None);
    }
}
