// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use regex::Regex;
use serde::{Deserialize, Deserializer};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Deserialize, PartialEq)]
struct WrappedString(String);

// deserialize String
pub fn str<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = String::deserialize(d)?;
    match self::is_env_var(&val) {
        true => Ok(self::get_env_var(&val)),
        false => Ok(val),
    }
}

// deserialize wrapped Option<String>
pub fn opt_str<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<WrappedString>::deserialize(d).map(|opt: Option<WrappedString>| {
        opt.map(|x: WrappedString| {
            let val = x.0;
            match self::is_env_var(&val) {
                true => self::get_env_var(&val),
                false => val,
            }
        })
    })
}

// deserialize SocketAddr
// It deserializes visitor as a String and after that it parses it to SocketAddr
pub fn socket_addr<'de, D>(d: D) -> Result<SocketAddr, D::Error>
where
    D: Deserializer<'de>,
{
    let val = String::deserialize(d)?;
    match self::is_env_var(&val) {
        true => Ok(self::get_env_var(&val).parse().unwrap()),
        false => Ok(val.parse().unwrap()),
    }
}

// deserialize PathBuf
// It deserializes visitor as a String and after that it parses it to PathBuf
pub fn path_buf<'de, D>(d: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let val = String::deserialize(d)?;
    match self::is_env_var(&val) {
        true => Ok(PathBuf::from(self::get_env_var(&val))),
        false => Ok(PathBuf::from(val)),
    }
}

// check using regex if provided visitor contains env variable
// pattern - ${env.VARIABLE}
fn is_env_var(s: &str) -> bool {
    let re = Regex::new(r"^\$\{env\.\w+\}$").expect("env_var: regex is invalid");
    re.is_match(s)
}

// parses visitor to varaible key and read variable using std::env
fn get_env_var(wrapped_key: &str) -> String {
    let key: String = String::from(wrapped_key)
        .drain(6..(wrapped_key.len() - 1))
        .collect();
    std::env::var(key.clone()).expect(&format!("env_var: variable '{}' is not set", key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_if_visitor_contains_env_var_pattern() {
        assert_eq!(self::is_env_var("${env.XXX}"), true);
        assert_eq!(self::is_env_var("${env.XXX"), false);
        assert_eq!(self::is_env_var("${env.XXX}a"), false);
        assert_eq!(self::is_env_var("a${env.XXX}"), false);
        assert_eq!(self::is_env_var("{env.XXX}"), false);
        assert_eq!(self::is_env_var("$env.XXX}"), false);
        assert_eq!(self::is_env_var("${envXXX}"), false);
        assert_eq!(self::is_env_var("${.XXX}"), false);
        assert_eq!(self::is_env_var("${XXX}"), false);
    }

    #[test]
    fn get_env_variable() {
        std::env::set_var("TEST", "test");
        assert_eq!(self::get_env_var("${env.TEST}"), "test");
        std::env::remove_var("TEST");
    }
}
