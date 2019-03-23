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

    pub fn to_str(&self) -> &'static str {
        match *self {
            ChannelMode::Search => "search",
            ChannelMode::Ingest => "ingest",
            ChannelMode::Control => "control",
        }
    }
}
