//! Domain ID generation and resolution
//!
//! All IDs use the format: `{6-char-hex}-{type}-{slug}`
//! Example: `019430-plan-add-oauth`

use std::collections::HashMap;

/// Generate a domain ID from type and title
pub fn generate_id(domain_type: &str, title: &str) -> String {
    let uuid = uuid::Uuid::now_v7();
    let hex_prefix = &uuid.to_string()[..6];
    let slug = slugify(title);
    format!("{}-{}-{}", hex_prefix, domain_type, slug)
}

/// Slugify a title for use in IDs
fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        // Strip apostrophes entirely, replace other non-alphanumeric with hyphens
        .filter_map(|c| {
            if c.is_alphanumeric() {
                Some(c)
            } else if c == '\'' || c == '\u{2019}' || c == '\u{2018}' {
                None // Strip apostrophes (straight and curly)
            } else {
                Some('-')
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Domain ID wrapper for type-safe ID handling
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DomainId(String);

impl DomainId {
    /// Create a new domain ID from type and title
    pub fn new(domain_type: &str, title: &str) -> Self {
        Self(generate_id(domain_type, title))
    }

    /// Create from an existing ID string
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get the hex prefix (first 6 chars)
    pub fn hex_prefix(&self) -> &str {
        &self.0[..6]
    }

    /// Get the full ID string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the slug portion (after type)
    pub fn slug(&self) -> Option<&str> {
        // Format: {hex}-{type}-{slug}
        let parts: Vec<&str> = self.0.splitn(3, '-').collect();
        parts.get(2).copied()
    }

    /// Get the type portion
    pub fn domain_type(&self) -> Option<&str> {
        let parts: Vec<&str> = self.0.splitn(3, '-').collect();
        parts.get(1).copied()
    }
}

impl std::fmt::Display for DomainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for DomainId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DomainId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for DomainId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for DomainId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for DomainId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self(s))
    }
}

/// ID resolution for partial matches
pub struct IdResolver<'a> {
    ids: &'a HashMap<String, String>, // id -> display name
}

impl<'a> IdResolver<'a> {
    pub fn new(ids: &'a HashMap<String, String>) -> Self {
        Self { ids }
    }

    /// Resolve a partial reference to a full ID
    ///
    /// Returns:
    /// - Ok(Some(id)) if exactly one match
    /// - Ok(None) if no matches
    /// - Err with candidates if ambiguous
    pub fn resolve(&self, reference: &str) -> Result<Option<String>, Vec<String>> {
        let matches: Vec<String> = self
            .ids
            .keys()
            .filter(|id| Self::matches(id, reference))
            .cloned()
            .collect();

        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches.into_iter().next().unwrap())),
            _ => Err(matches),
        }
    }

    /// Check if an ID matches a reference
    fn matches(id: &str, reference: &str) -> bool {
        // Exact match
        if id == reference {
            return true;
        }

        // Hex prefix match (first 6 chars)
        if id.starts_with(reference) {
            return true;
        }

        // Slug contains match
        if let Some(slug_start) = id.find('-') {
            let slug_part = &id[slug_start + 1..];
            if slug_part.contains(reference) {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_id() {
        let id = generate_id("plan", "Add OAuth Authentication");
        assert!(id.len() > 10);
        assert!(id.contains("-plan-"));
        assert!(id.contains("add-oauth-authentication"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Add OAuth!"), "add-oauth");
        assert_eq!(slugify("Multiple   Spaces"), "multiple-spaces");
        assert_eq!(slugify("CamelCase"), "camelcase");
        // Apostrophes should be stripped, not converted to hyphens
        assert_eq!(slugify("here's a test"), "heres-a-test");
        assert_eq!(slugify("don't stop"), "dont-stop");
        assert_eq!(slugify("it's working"), "its-working");
    }

    #[test]
    fn test_domain_id_parts() {
        let id = DomainId::from_string("019430-plan-add-oauth".to_string());
        assert_eq!(id.hex_prefix(), "019430");
        assert_eq!(id.domain_type(), Some("plan"));
        assert_eq!(id.slug(), Some("add-oauth"));
    }

    #[test]
    fn test_id_resolver_exact() {
        let mut ids = HashMap::new();
        ids.insert("019430-plan-add-oauth".to_string(), "Add OAuth".to_string());
        ids.insert("019431-spec-oauth-db".to_string(), "OAuth DB Schema".to_string());

        let resolver = IdResolver::new(&ids);
        assert_eq!(
            resolver.resolve("019430-plan-add-oauth").unwrap(),
            Some("019430-plan-add-oauth".to_string())
        );
    }

    #[test]
    fn test_id_resolver_hex_prefix() {
        let mut ids = HashMap::new();
        ids.insert("019430-plan-add-oauth".to_string(), "Add OAuth".to_string());
        ids.insert("019431-spec-oauth-db".to_string(), "OAuth DB Schema".to_string());

        let resolver = IdResolver::new(&ids);
        assert_eq!(
            resolver.resolve("019430").unwrap(),
            Some("019430-plan-add-oauth".to_string())
        );
    }

    #[test]
    fn test_id_resolver_slug_match() {
        let mut ids = HashMap::new();
        ids.insert("019430-plan-add-oauth".to_string(), "Add OAuth".to_string());
        ids.insert("019431-spec-oauth-db".to_string(), "OAuth DB Schema".to_string());

        let resolver = IdResolver::new(&ids);
        assert_eq!(
            resolver.resolve("oauth-db").unwrap(),
            Some("019431-spec-oauth-db".to_string())
        );
    }

    #[test]
    fn test_id_resolver_ambiguous() {
        let mut ids = HashMap::new();
        ids.insert("019430-plan-add-oauth".to_string(), "Add OAuth".to_string());
        ids.insert("019431-spec-oauth-db".to_string(), "OAuth DB Schema".to_string());
        ids.insert("019432-spec-oauth-api".to_string(), "OAuth API".to_string());

        let resolver = IdResolver::new(&ids);
        let result = resolver.resolve("oauth");
        assert!(result.is_err());
        let candidates = result.unwrap_err();
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn test_id_resolver_no_match() {
        let mut ids = HashMap::new();
        ids.insert("019430-plan-add-oauth".to_string(), "Add OAuth".to_string());

        let resolver = IdResolver::new(&ids);
        assert_eq!(resolver.resolve("nonexistent").unwrap(), None);
    }
}
