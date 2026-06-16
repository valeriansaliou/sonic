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
