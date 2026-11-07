// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub enum ChannelMode {
    Search,
    Ingest,
    Control,
}

impl ChannelMode {
    pub fn from_str(value: &str) -> Result<Self, ()> {
        match value {
            "search" => Ok(ChannelMode::Search),
            "ingest" => Ok(ChannelMode::Ingest),
            "control" => Ok(ChannelMode::Control),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelMode::Search => write!(f, "search"),
            ChannelMode::Ingest => write!(f, "ingest"),
            ChannelMode::Control => write!(f, "control"),
        }
    }
}
