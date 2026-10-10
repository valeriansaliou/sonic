// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::str::FromStr;

use serde::Deserialize;

pub(crate) mod byte_amount {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct ByteAmount<const MAX_SCALE: u8, N>(N);

    #[derive(Debug, thiserror::Error)]
    pub enum ParseMeasurementError<N: std::fmt::Display, UnitError> {
        #[error("Invalid quantity")]
        InvalidQuantity(#[source] std::num::ParseIntError),

        #[error("Invalid unit")]
        InvalidUnit(#[source] UnitError),

        #[error("Value too large (max: {0})")]
        TooLarge(N),
    }

    pub(crate) trait PrimitiveNumber:
        Sized
        + std::fmt::Debug
        + std::fmt::Display
        + TryFrom<u64>
        + FromStr<Err = std::num::ParseIntError>
        + for<'de> Deserialize<'de>
    {
        fn checked_mul(self, rhs: Self) -> Option<Self>;

        fn max() -> Self;
    }

    macro_rules! impl_primitive_number {
        ($($t:ty),+) => {
            $(impl PrimitiveNumber for $t {
                fn checked_mul(self, rhs: Self) -> Option<Self> {
                    self.checked_mul(rhs)
                }

                fn max() -> Self { Self::MAX }
            })+
        };
    }

    impl_primitive_number!(u8, u16, u32, u64, u128);

    impl<const MAX_SCALE: u8, N: PrimitiveNumber> ByteAmount<MAX_SCALE, N> {
        pub(crate) fn parse<'a>(
            s: &'a str,
        ) -> Result<Self, ParseMeasurementError<N, UnknownByteScaleError<'a>>> {
            use ParseMeasurementError as ParseError;

            let (magnitude, scale) = crate::util::split_leading_digits(s);

            let magnitude = N::from_str(magnitude).map_err(ParseError::InvalidQuantity)?;
            let scale = ByteScale::parse(scale, MAX_SCALE).map_err(ParseError::InvalidUnit)?;

            let factor =
                N::try_from(scale.factor()).map_err(|_e| ParseError::TooLarge(N::max()))?;

            let res = (magnitude.checked_mul(factor)).ok_or(ParseError::TooLarge(N::max()))?;

            Ok(Self(res))
        }

        pub(crate) fn value(self) -> N {
            self.0
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u64)]
    pub(crate) enum ByteScale {
        Byte = 1,
        KibiByte = 1024,
        MebiByte = 1024 * 1024,
        GibiByte = 1024 * 1024 * 1024,
        TebiByte = 1024 * 1024 * 1024 * 1024,
        KiloByte = 1000,
        MegaByte = 1000 * 1000,
        GigaByte = 1000 * 1000 * 1000,
        TeraByte = 1000 * 1000 * 1000 * 1000,
    }

    #[derive(Debug)]
    pub(crate) struct UnknownByteScaleError<'a>(&'a str, u8);

    impl std::fmt::Display for UnknownByteScaleError<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self(found, max_power) = self;

            let mut expected = String::with_capacity(55);
            expected.push_str(r#""B""#);
            if *max_power <= 3 {
                expected.push_str(r#", "KB", "KiB""#);
            }
            if *max_power <= 6 {
                expected.push_str(r#", "MB", "MiB""#);
            }
            if *max_power <= 9 {
                expected.push_str(r#", "GB", "GiB""#);
            }
            if *max_power <= 12 {
                expected.push_str(r#", "TB", "TiB""#);
            }

            write!(
                f,
                "Unknown byte scale factor {found:?}; expected one of: {expected}"
            )
        }
    }

    impl std::error::Error for UnknownByteScaleError<'_> {}

    impl ByteScale {
        pub(crate) fn parse<'a>(
            s: &'a str,
            max_power: u8,
        ) -> Result<Self, UnknownByteScaleError<'a>> {
            match s.to_ascii_lowercase().as_str() {
                "b" => Ok(Self::Byte),
                "kib" if max_power <= 3 => Ok(Self::KibiByte),
                "mib" if max_power <= 6 => Ok(Self::MebiByte),
                "gib" if max_power <= 9 => Ok(Self::GibiByte),
                "tib" if max_power <= 12 => Ok(Self::TebiByte),
                "kb" if max_power <= 3 => Ok(Self::KiloByte),
                "mb" if max_power <= 6 => Ok(Self::MegaByte),
                "gb" if max_power <= 9 => Ok(Self::GigaByte),
                "tb" if max_power <= 12 => Ok(Self::TeraByte),
                _ => Err(UnknownByteScaleError(s, max_power)),
            }
        }

        pub(crate) fn factor(self) -> u64 {
            self as u64
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum NumberOrString<N> {
        Number(N),
        String(String),
    }

    pub(crate) fn deserialize<'de, const MAX_SCALE: u8, D, N: PrimitiveNumber>(
        deserializer: D,
    ) -> Result<ByteAmount<MAX_SCALE, N>, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        match NumberOrString::<N>::deserialize(deserializer)? {
            NumberOrString::Number(n) => Ok(ByteAmount(n)),
            NumberOrString::String(s) => ByteAmount::parse(&s).map_err(serde::de::Error::custom),
        }
    }
}
