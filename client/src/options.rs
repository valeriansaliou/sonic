// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! options {
    ($($opt:expr ),+) => {
        &[$(&$opt,)+]
    };
}

#[derive(Debug, Clone, Copy)]
pub struct Limit(pub usize);

impl std::fmt::Display for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LIMIT({})", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Lang<'a>(pub &'a str);

impl<'a> std::fmt::Display for Lang<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LANG({})", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Offset(pub usize);

impl std::fmt::Display for Offset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OFFSET({})", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Timestamp(pub u64);

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TS({})", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FromTimestamp(pub u64);

impl std::fmt::Display for FromTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FROM({})", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToTimestamp(pub u64);

impl std::fmt::Display for ToTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TO({})", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Metadata(String);

impl Metadata {
    pub fn new(value: &serde_json::Value) -> std::io::Result<Self> {
        use base64::Engine;
        if !value.is_object() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "metadata must be a JSON object",
            ));
        }
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(value)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?,
        );
        Ok(Self(encoded))
    }
}

impl std::fmt::Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "META({})", self.0)
    }
}
