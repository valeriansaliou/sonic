// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! io_error {
    ($error:expr) => {
        io::Error::new(io::ErrorKind::Other, $error)
    };
}
