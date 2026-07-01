//! Decide which account a given directory routes to.
//!
//! Rule: among all rules whose `path_prefix` is an ancestor of (or equal to)
//! the directory, the **longest** prefix wins. This lets a nested folder
//! override a broader one (e.g. `~/Dev/work-thing` → work while `~/Dev` → personal).

use crate::config::{normalize_prefix, Config, Rule};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Match<'a> {
    pub rule: &'a Rule,
    pub account_id: &'a str,
}

/// Return the best-matching rule for `dir`, if any.
pub fn resolve<'a>(config: &'a Config, dir: &Path) -> Option<Match<'a>> {
    let dir_norm = normalize_prefix(&dir.to_string_lossy());

    let mut best: Option<&Rule> = None;
    for rule in &config.rules {
        if path_is_under(&dir_norm, &rule.path_prefix) {
            match best {
                Some(b) if b.path_prefix.len() >= rule.path_prefix.len() => {}
                _ => best = Some(rule),
            }
        }
    }

    best.map(|rule| Match {
        rule,
        account_id: &rule.account_id,
    })
}

/// Is `dir` equal to, or nested inside, `prefix`? Both are normalized absolute
/// paths. We compare on path component boundaries so `/Users/alex/22apps` does
/// NOT match a prefix of `/Users/alex/22` (which would be a different folder).
fn path_is_under(dir: &str, prefix: &str) -> bool {
    if dir == prefix {
        return true;
    }
    if prefix == "/" {
        return dir.starts_with('/');
    }
    // dir must start with "<prefix>/"
    let mut needle = String::with_capacity(prefix.len() + 1);
    needle.push_str(prefix);
    needle.push('/');
    dir.starts_with(&needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Account, Config};

    fn cfg() -> Config {
        let mut c = Config::default();
        c.accounts.push(Account {
            id: "personal".into(),
            label: "Personal".into(),
            provider: "vercel".into(),
            username: None,
            email: None,
            has_token: false,
        });
        c.accounts.push(Account {
            id: "work".into(),
            label: "Work".into(),
            provider: "vercel".into(),
            username: None,
            email: None,
            has_token: false,
        });
        c.upsert_rule("/Users/alex/Dev".into(), "personal".into());
        c.upsert_rule("/Users/alex/22apps".into(), "work".into());
        c.upsert_rule("/Users/alex/Dev/client-x".into(), "work".into());
        c
    }

    #[test]
    fn matches_nested_dir() {
        let c = cfg();
        let m = resolve(&c, Path::new("/Users/alex/Dev/portfolio/site")).unwrap();
        assert_eq!(m.account_id, "personal");
    }

    #[test]
    fn longest_prefix_wins() {
        let c = cfg();
        let m = resolve(&c, Path::new("/Users/alex/Dev/client-x/app")).unwrap();
        assert_eq!(m.account_id, "work");
    }

    #[test]
    fn does_not_match_sibling_with_shared_text() {
        let c = cfg();
        // "/Users/alex/22apps" must not be matched by a "/Users/alex/22" prefix
        let mut c2 = Config::default();
        c2.upsert_rule("/Users/alex/22".into(), "work".into());
        assert!(resolve(&c2, Path::new("/Users/alex/22apps")).is_none());
        // but the real rule still works
        let m = resolve(&c, Path::new("/Users/alex/22apps")).unwrap();
        assert_eq!(m.account_id, "work");
    }

    #[test]
    fn no_match_outside() {
        let c = cfg();
        assert!(resolve(&c, Path::new("/tmp/whatever")).is_none());
    }
}
