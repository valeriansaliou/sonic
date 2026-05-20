// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub mod env_var {
    use regex::Regex;
    use serde::{Deserialize, Deserializer};
    use std::net::SocketAddr;
    use std::path::PathBuf;

    #[derive(Deserialize, PartialEq)]
    struct WrappedString(String);

    pub fn str<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        match is_env_var(&value) {
            true => Ok(get_env_var(&value)),
            false => Ok(value),
        }
    }

    pub fn opt_str<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<WrappedString>::deserialize(deserializer).map(|option: Option<WrappedString>| {
            option.map(|wrapped: WrappedString| {
                let value = wrapped.0;

                match is_env_var(&value) {
                    true => get_env_var(&value),
                    false => value,
                }
            })
        })
    }

    pub fn socket_addr<'de, D>(deserializer: D) -> Result<SocketAddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        match is_env_var(&value) {
            true => Ok(get_env_var(&value).parse().unwrap()),
            false => Ok(value.parse().unwrap()),
        }
    }

    pub fn path_buf<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        match is_env_var(&value) {
            true => Ok(PathBuf::from(get_env_var(&value))),
            false => Ok(PathBuf::from(value)),
        }
    }

    fn is_env_var(value: &str) -> bool {
        Regex::new(r"^\$\{env\.\w+\}$")
            .expect("env_var: regex is invalid")
            .is_match(value)
    }

    fn get_env_var(wrapped_key: &str) -> String {
        let key: String = String::from(wrapped_key)
            .drain(6..(wrapped_key.len() - 1))
            .collect();

        // NOTE: While we could deprecate the `${env.*}` syntax now that Sonic has
        //   first-class support for environment variables, it would force people
        //   to use the Sonic naming convention and potentially duplicate some
        //   variables. For better UX, let’s keep it that way. It doesn’t require
        //   dependencies nor bloat the code so it’s acceptable.

        std::env::var(&key).unwrap_or_else(|_| panic!("env_var: variable '{key}' is not set"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn it_checks_environment_variable_patterns() {
            assert!(is_env_var("${env.XXX}"));
            assert!(!is_env_var("${env.XXX"));
            assert!(!is_env_var("${env.XXX}a"));
            assert!(!is_env_var("a${env.XXX}"));
            assert!(!is_env_var("{env.XXX}"));
            assert!(!is_env_var("$env.XXX}"));
            assert!(!is_env_var("${envXXX}"));
            assert!(!is_env_var("${.XXX}"));
            assert!(!is_env_var("${XXX}"));
        }

        #[test]
        fn it_gets_environment_variable() {
            unsafe { std::env::set_var("TEST", "test") };

            assert_eq!(get_env_var("${env.TEST}"), "test");

            unsafe { std::env::remove_var("TEST") };
        }
    }
}
