//! Strongly-typed canonical plugin identifier and path security validation.
//!
//! Enforces single path segment isolation and path traversal prevention across
//! all subsystems (PluginDataDir, PluginConfigStore, Supervisor, etc.).

use std::fmt;
use std::str::FromStr;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Error returned when a plugin identifier fails path safety or syntax validation.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum PluginIdError {
    /// Identifier is empty.
    #[error("Plugin identifier cannot be empty")]
    Empty,
    /// Identifier matches forbidden relative path tokens ('.' or '..').
    #[error("Plugin identifier cannot be '.' or '..'")]
    ReservedPathToken,
    /// Identifier contains path separator characters ('/' or '\\').
    #[error("Plugin identifier cannot contain path separators ('/' or '\\')")]
    ContainsPathSeparator,
    /// Identifier contains a NUL byte.
    #[error("Plugin identifier cannot contain NUL bytes")]
    ContainsNullByte,
    /// Identifier contains non-printable or forbidden control characters.
    #[error("Plugin identifier contains invalid control characters")]
    InvalidCharacters,
}

/// Strongly-typed canonical plugin identifier.
///
/// Guarantees that the contained string is safe to use as a single filesystem directory name
/// without directory traversal vulnerabilities or cross-tenant escaping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    /// Validates and parses a raw string slice into a verified [`PluginId`].
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, PluginIdError> {
        let s = raw.as_ref();
        Self::validate(s)?;
        Ok(Self(s.to_string()))
    }

    /// Performs strict validation on a potential plugin identifier without allocating.
    pub fn validate(raw: &str) -> Result<(), PluginIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(PluginIdError::Empty);
        }
        if raw != trimmed {
            return Err(PluginIdError::InvalidCharacters);
        }
        if raw == "." || raw == ".." {
            return Err(PluginIdError::ReservedPathToken);
        }
        if raw.contains('/') || raw.contains('\\') {
            return Err(PluginIdError::ContainsPathSeparator);
        }
        if raw.contains('\0') {
            return Err(PluginIdError::ContainsNullByte);
        }
        if raw.chars().any(|c| c.is_control()) {
            return Err(PluginIdError::InvalidCharacters);
        }
        Ok(())
    }

    /// Returns the raw identifier string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the underlying `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for PluginId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::path::Path> for PluginId {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for PluginId {
    type Err = PluginIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for PluginId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}
