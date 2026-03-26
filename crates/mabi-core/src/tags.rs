//! Device tagging system.
//!
//! Provides a unified tagging mechanism for organizing and filtering devices
//! across all protocols. Tags support:
//!
//! - **Key-Value Tags**: Structured metadata (e.g., `location=building-a`)
//! - **Labels**: Set-based tags for grouping (e.g., `production`, `critical`)
//!
//! # Examples
//!
//! ```rust
//! use mabi_core::tags::Tags;
//!
//! let tags = Tags::new()
//!     .with_tag("location", "building-a")
//!     .with_tag("floor", "3")
//!     .with_label("hvac")
//!     .with_label("critical");
//!
//! assert!(tags.has_label("hvac"));
//! assert_eq!(tags.get("location"), Some("building-a"));
//! assert!(tags.matches_selector(&[("location", "building-a")]));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Device tags for organization and filtering.
///
/// Tags provide a flexible way to categorize and filter devices:
/// - Use key-value tags for structured metadata
/// - Use labels for simple grouping
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tags {
    /// Key-value tags (e.g., location=building-a, zone=hvac).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    tags: HashMap<String, String>,

    /// Simple labels for grouping (e.g., production, critical).
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    labels: HashSet<String>,
}

impl Tags {
    /// Create empty tags.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create tags from a HashMap.
    pub fn from_map(tags: HashMap<String, String>) -> Self {
        Self {
            tags,
            labels: HashSet::new(),
        }
    }

    /// Create tags from key-value pairs.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            tags: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            labels: HashSet::new(),
        }
    }

    /// Add a key-value tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.labels.insert(label.into());
        self
    }

    /// Add multiple labels.
    pub fn with_labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.labels.extend(labels.into_iter().map(|s| s.into()));
        self
    }

    /// Add multiple key-value tags.
    pub fn with_tags<I, K, V>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.tags
            .extend(tags.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Insert a key-value tag (mutable).
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.tags.insert(key.into(), value.into());
    }

    /// Add a label (mutable).
    pub fn add_label(&mut self, label: impl Into<String>) {
        self.labels.insert(label.into());
    }

    /// Remove a tag by key.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.tags.remove(key)
    }

    /// Remove a label.
    pub fn remove_label(&mut self, label: &str) -> bool {
        self.labels.remove(label)
    }

    /// Get a tag value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|s| s.as_str())
    }

    /// Check if a tag key exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.tags.contains_key(key)
    }

    /// Check if a label exists.
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.contains(label)
    }

    /// Get all key-value tags.
    pub fn tags(&self) -> &HashMap<String, String> {
        &self.tags
    }

    /// Get all labels.
    pub fn labels(&self) -> &HashSet<String> {
        &self.labels
    }

    /// Check if tags are empty.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.labels.is_empty()
    }

    /// Get total count of tags and labels.
    pub fn len(&self) -> usize {
        self.tags.len() + self.labels.len()
    }

    /// Merge another Tags into this one.
    /// Existing keys are overwritten.
    pub fn merge(&mut self, other: Tags) {
        self.tags.extend(other.tags);
        self.labels.extend(other.labels);
    }

    /// Create a merged copy with another Tags.
    pub fn merged_with(mut self, other: Tags) -> Self {
        self.merge(other);
        self
    }

    /// Check if all selector key-value pairs match.
    ///
    /// # Example
    /// ```rust
    /// use mabi_core::tags::Tags;
    ///
    /// let tags = Tags::new()
    ///     .with_tag("location", "building-a")
    ///     .with_tag("floor", "3");
    ///
    /// assert!(tags.matches_selector(&[("location", "building-a")]));
    /// assert!(tags.matches_selector(&[("location", "building-a"), ("floor", "3")]));
    /// assert!(!tags.matches_selector(&[("location", "building-b")]));
    /// ```
    pub fn matches_selector(&self, selector: &[(&str, &str)]) -> bool {
        selector.iter().all(|(k, v)| self.get(k) == Some(*v))
    }

    /// Check if any of the specified labels exist.
    pub fn has_any_label<'a, I>(&self, labels: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        labels.into_iter().any(|l| self.labels.contains(l))
    }

    /// Check if all of the specified labels exist.
    pub fn has_all_labels<'a, I>(&self, labels: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        labels.into_iter().all(|l| self.labels.contains(l))
    }

    /// Iterate over all key-value tags.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.tags.iter()
    }

    /// Iterate over all labels.
    pub fn iter_labels(&self) -> impl Iterator<Item = &String> {
        self.labels.iter()
    }
}

/// Builder for creating Tags with fluent API.
#[derive(Debug, Default)]
pub struct TagsBuilder {
    tags: HashMap<String, String>,
    labels: HashSet<String>,
}

impl TagsBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a key-value tag.
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.labels.insert(label.into());
        self
    }

    /// Build the Tags.
    pub fn build(self) -> Tags {
        Tags {
            tags: self.tags,
            labels: self.labels,
        }
    }
}

/// Parse tags from CLI format: "key=value" or "label" (no '=').
///
/// # Examples
/// ```rust
/// use mabi_core::tags::parse_tag_string;
///
/// assert_eq!(parse_tag_string("location=building-a"), Ok(("location".to_string(), Some("building-a".to_string()))));
/// assert_eq!(parse_tag_string("critical"), Ok(("critical".to_string(), None)));
/// assert!(parse_tag_string("").is_err());
/// ```
pub fn parse_tag_string(s: &str) -> Result<(String, Option<String>), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Empty tag string".to_string());
    }

    if let Some(pos) = s.find('=') {
        let key = s[..pos].trim();
        let value = s[pos + 1..].trim();

        if key.is_empty() {
            return Err("Empty tag key".to_string());
        }

        Ok((key.to_string(), Some(value.to_string())))
    } else {
        // Label (no value)
        Ok((s.to_string(), None))
    }
}

/// Parse multiple tag strings into Tags.
///
/// # Examples
/// ```rust
/// use mabi_core::tags::parse_tags;
///
/// let tags = parse_tags(&["location=building-a", "floor=3", "critical"]).unwrap();
/// assert_eq!(tags.get("location"), Some("building-a"));
/// assert_eq!(tags.get("floor"), Some("3"));
/// assert!(tags.has_label("critical"));
/// ```
pub fn parse_tags<I, S>(tag_strings: I) -> Result<Tags, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut tags = Tags::new();

    for s in tag_strings {
        let (key, value) = parse_tag_string(s.as_ref())?;
        match value {
            Some(v) => tags.insert(key, v),
            None => tags.add_label(key),
        }
    }

    Ok(tags)
}

/// Extension trait for types that can have tags.
pub trait Taggable {
    /// Get tags reference.
    fn tags(&self) -> &Tags;

    /// Get mutable tags reference.
    fn tags_mut(&mut self) -> &mut Tags;

    /// Check if has a specific tag value.
    fn has_tag(&self, key: &str, value: &str) -> bool {
        self.tags().get(key) == Some(value)
    }

    /// Check if has a specific label.
    fn has_label(&self, label: &str) -> bool {
        self.tags().has_label(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tags_creation() {
        let tags = Tags::new()
            .with_tag("location", "building-a")
            .with_tag("floor", "3")
            .with_label("hvac")
            .with_label("critical");

        assert_eq!(tags.get("location"), Some("building-a"));
        assert_eq!(tags.get("floor"), Some("3"));
        assert!(tags.has_label("hvac"));
        assert!(tags.has_label("critical"));
        assert!(!tags.has_label("nonexistent"));
    }

    #[test]
    fn test_tags_from_pairs() {
        let tags = Tags::from_pairs([("key1", "value1"), ("key2", "value2")]);
        assert_eq!(tags.get("key1"), Some("value1"));
        assert_eq!(tags.get("key2"), Some("value2"));
    }

    #[test]
    fn test_tags_merge() {
        let mut tags1 = Tags::new().with_tag("a", "1").with_label("x");

        let tags2 = Tags::new()
            .with_tag("b", "2")
            .with_tag("a", "overwritten")
            .with_label("y");

        tags1.merge(tags2);

        assert_eq!(tags1.get("a"), Some("overwritten"));
        assert_eq!(tags1.get("b"), Some("2"));
        assert!(tags1.has_label("x"));
        assert!(tags1.has_label("y"));
    }

    #[test]
    fn test_tags_selector() {
        let tags = Tags::new()
            .with_tag("location", "building-a")
            .with_tag("floor", "3")
            .with_tag("zone", "hvac");

        assert!(tags.matches_selector(&[("location", "building-a")]));
        assert!(tags.matches_selector(&[("location", "building-a"), ("floor", "3")]));
        assert!(!tags.matches_selector(&[("location", "building-b")]));
        assert!(!tags.matches_selector(&[("nonexistent", "value")]));
    }

    #[test]
    fn test_tags_label_queries() {
        let tags = Tags::new()
            .with_label("production")
            .with_label("critical")
            .with_label("hvac");

        assert!(tags.has_any_label(["production", "test"].iter().copied()));
        assert!(!tags.has_any_label(["test", "dev"].iter().copied()));
        assert!(tags.has_all_labels(["production", "critical"].iter().copied()));
        assert!(!tags.has_all_labels(["production", "test"].iter().copied()));
    }

    #[test]
    fn test_parse_tag_string() {
        assert_eq!(
            parse_tag_string("location=building-a"),
            Ok(("location".to_string(), Some("building-a".to_string())))
        );
        assert_eq!(
            parse_tag_string("critical"),
            Ok(("critical".to_string(), None))
        );
        assert_eq!(
            parse_tag_string("key="),
            Ok(("key".to_string(), Some("".to_string())))
        );
        assert!(parse_tag_string("").is_err());
        assert!(parse_tag_string("=value").is_err());
    }

    #[test]
    fn test_parse_tags() {
        let tags =
            parse_tags(&["location=building-a", "floor=3", "critical", "production"]).unwrap();

        assert_eq!(tags.get("location"), Some("building-a"));
        assert_eq!(tags.get("floor"), Some("3"));
        assert!(tags.has_label("critical"));
        assert!(tags.has_label("production"));
    }

    #[test]
    fn test_tags_builder() {
        let tags = TagsBuilder::new()
            .tag("env", "prod")
            .tag("region", "us-west")
            .label("monitored")
            .build();

        assert_eq!(tags.get("env"), Some("prod"));
        assert!(tags.has_label("monitored"));
    }

    #[test]
    fn test_tags_serialization() {
        let tags = Tags::new()
            .with_tag("location", "building-a")
            .with_label("critical");

        let json = serde_json::to_string(&tags).unwrap();
        let deserialized: Tags = serde_json::from_str(&json).unwrap();

        assert_eq!(tags, deserialized);
    }

    #[test]
    fn test_empty_tags_serialization() {
        let tags = Tags::new();
        let json = serde_json::to_string(&tags).unwrap();
        // Empty maps/sets should be skipped
        assert_eq!(json, "{}");
    }
}
