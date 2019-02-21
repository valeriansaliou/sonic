// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;

pub fn unescape(text: &str) -> String {
    let mut queue: VecDeque<_> = String::from(text).chars().collect();
    let mut unescaped = String::new();

    while let Some(character) = queue.pop_front() {
        if character != '\\' {
            unescaped.push(character);

            continue;
        }

        match queue.pop_front() {
            Some('n') => unescaped.push('\n'),
            Some('\"') => unescaped.push('\"'),
            _ => unescaped.push(character),
        };
    }

    unescaped
}
