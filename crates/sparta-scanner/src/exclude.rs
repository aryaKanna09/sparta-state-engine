//! User-supplied exclusion rules for the addressable SRF.
//!
//! DR copies, replication targets, and backup prefixes inflate raw redundancy
//! that no gateway deployment would actually deduplicate (they're written by
//! replication, not by the application). The addressable SRF excludes objects
//! matched by these rules from both numerator and denominator.

use anyhow::{bail, Result};

/// A `*`-wildcard pattern matched against `bucket/key`.
#[derive(Debug, Clone)]
pub struct PrefixRule {
    pattern: String,
}

impl PrefixRule {
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
        }
    }

    pub fn matches(&self, bucket: &str, key: &str) -> bool {
        let target = format!("{bucket}/{key}");
        wildcard_match(&self.pattern, &target) || wildcard_match(&self.pattern, key)
    }
}

/// Simple glob: `*` matches any run of characters (including `/`).
/// Patterns without a `*` are treated as prefixes.
fn wildcard_match(pattern: &str, s: &str) -> bool {
    if !pattern.contains('*') {
        return s.starts_with(pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !s[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if let Some(found) = s[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    // If the pattern doesn't end with '*', the last literal must anchor at end.
    if let Some(last) = parts.last() {
        if !last.is_empty() && !pattern.ends_with('*') {
            return s.ends_with(last);
        }
    }
    true
}

/// A `key=value` tag rule, checked via GetObjectTagging on sampled objects.
#[derive(Debug, Clone)]
pub struct TagRule {
    pub key: String,
    pub value: String,
}

impl TagRule {
    pub fn parse(s: &str) -> Result<Self> {
        let Some((k, v)) = s.split_once('=') else {
            bail!("tag rule must be key=value, got {s:?}");
        };
        Ok(Self {
            key: k.to_string(),
            value: v.to_string(),
        })
    }

    pub fn matches(&self, tags: &[(String, String)]) -> bool {
        tags.iter().any(|(k, v)| *k == self.key && *v == self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prefix() {
        let r = PrefixRule::new("dr-copies/");
        assert!(r.matches("bkt", "dr-copies/x/y.tar"));
        assert!(!r.matches("bkt", "live/dr-copies/x"));
    }

    #[test]
    fn bucket_qualified_prefix() {
        let r = PrefixRule::new("backup-bucket/");
        assert!(r.matches("backup-bucket", "anything"));
        assert!(!r.matches("live-bucket", "anything"));
    }

    #[test]
    fn wildcard_patterns() {
        let r = PrefixRule::new("*/replica/*");
        assert!(r.matches("bkt", "a/replica/obj"));
        assert!(!r.matches("bkt", "a/primary/obj"));
        let suffix = PrefixRule::new("*.bak");
        assert!(suffix.matches("bkt", "x/y.bak"));
        assert!(!suffix.matches("bkt", "x/y.bak.live"));
    }

    #[test]
    fn tag_rules() {
        let t = TagRule::parse("replication=true").unwrap();
        assert!(t.matches(&[("replication".into(), "true".into())]));
        assert!(!t.matches(&[("replication".into(), "false".into())]));
        assert!(TagRule::parse("noequals").is_err());
    }
}
