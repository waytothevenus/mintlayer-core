// Copyright (c) 2023 RBB S.r.l
// opensource@mintlayer.org
// SPDX-License-Identifier: MIT
// Licensed under the MIT License;
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://github.com/mintlayer/mintlayer-core/blob/master/LICENSE
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::Display;

use crypto::random::Rng;
use serialization::{Decode, Encode, Error, Input};
use thiserror::Error;

use super::Amount;

const DENOMINATOR: u16 = 1000;

#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Encode, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct PerThousand(u16);

impl PerThousand {
    pub const fn new(value: u16) -> Option<Self> {
        if value <= DENOMINATOR {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn new_from_rng(rng: &mut impl Rng) -> Self {
        Self(rng.gen_range(0..=DENOMINATOR))
    }

    pub fn value(&self) -> u16 {
        self.0
    }

    pub const fn denominator(&self) -> u16 {
        DENOMINATOR
    }

    #[allow(clippy::float_arithmetic)]
    pub fn as_f64(&self) -> f64 {
        self.0 as f64 / DENOMINATOR as f64
    }

    pub fn from_decimal_str(s: &str) -> Option<Self> {
        // TODO: abstract from_fixedpoint_str() outside of Amount
        let amount = if s.trim().ends_with('%') {
            let s = s.trim_end_matches('%');
            Amount::from_fixedpoint_str(s, 1)?
        } else {
            Amount::from_fixedpoint_str(s, 3)?
        };
        let value: u16 = amount.into_atoms().try_into().ok()?;

        let result = Self::new(value)?;
        Some(result)
    }

    // Clap's value_parser requires a function that returns a Result, that's why we have it.
    // TODO: make from_decimal_str return Result instead?
    pub fn from_decimal_str_with_result(s: &str) -> Result<Self, PerThousandParseError> {
        Self::from_decimal_str(s).ok_or_else(|| PerThousandParseError {
            bad_value: s.to_owned(),
        })
    }
}

#[derive(Error, Debug)]
#[error("Incorrect per-thousand value: {bad_value}")]
pub struct PerThousandParseError {
    bad_value: String,
}

impl Decode for PerThousand {
    fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
        let decoded_value = u16::decode(input)?;
        Self::new(decoded_value).ok_or(
            serialization::Error::from("PerThousand deserialization failed")
                .chain(format!("With decoded value: {}", decoded_value)),
        )
    }
}

impl Display for PerThousand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}%",
            Amount::from_atoms(self.0.into()).into_fixedpoint_str(1)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crypto::random::Rng;
    use rstest::rstest;
    use test_utils::random::Seed;

    #[rstest]
    #[trace]
    #[case(Seed::from_entropy())]
    fn test_from_decimal_str(#[case] seed: Seed) {
        let mut rng = test_utils::random::make_seedable_rng(seed);
        for value in 0..=DENOMINATOR {
            let per_thousand = PerThousand::new(value).unwrap();
            let per_thousand_str =
                Amount::into_fixedpoint_str(Amount::from_atoms(value as u128), 3);
            let per_thousand_str_percent =
                Amount::into_fixedpoint_str(Amount::from_atoms(value as u128), 1) + "%";
            assert_eq!(
                PerThousand::from_decimal_str(&per_thousand_str).unwrap(),
                per_thousand
            );
            assert_eq!(
                PerThousand::from_decimal_str(&per_thousand_str_percent).unwrap(),
                per_thousand
            );
        }
        // test an invalid value
        {
            let value = rng.gen_range(1001..u16::MAX);
            let per_thousand_str =
                Amount::into_fixedpoint_str(Amount::from_atoms(value as u128), 3);
            let per_thousand_str_percent =
                Amount::into_fixedpoint_str(Amount::from_atoms(value as u128), 1) + "%";
            assert!(PerThousand::from_decimal_str(&per_thousand_str).is_none());
            assert!(PerThousand::from_decimal_str(&per_thousand_str_percent).is_none());
        }
    }

    #[test]
    fn test_to_string() {
        assert_eq!(PerThousand::new(1).unwrap().to_string(), "0.1%");
        assert_eq!(PerThousand::new(10).unwrap().to_string(), "1%");
        assert_eq!(PerThousand::new(100).unwrap().to_string(), "10%");
        assert_eq!(PerThousand::new(1000).unwrap().to_string(), "100%");

        assert_eq!(PerThousand::new(11).unwrap().to_string(), "1.1%");
        assert_eq!(PerThousand::new(23).unwrap().to_string(), "2.3%");
        assert_eq!(PerThousand::new(98).unwrap().to_string(), "9.8%");

        assert_eq!(PerThousand::new(311).unwrap().to_string(), "31.1%");
        assert_eq!(PerThousand::new(564).unwrap().to_string(), "56.4%");
        assert_eq!(PerThousand::new(827).unwrap().to_string(), "82.7%");
    }

    #[rstest]
    #[trace]
    #[case(Seed::from_entropy())]
    fn test_per_thousand(#[case] seed: Seed) {
        let mut rng = test_utils::random::make_seedable_rng(seed);

        assert!(PerThousand::new_from_rng(&mut rng).value() <= DENOMINATOR);

        assert_eq!(PerThousand::new(0).unwrap().value(), 0);
        assert_eq!(PerThousand::new(DENOMINATOR).unwrap().value(), DENOMINATOR);

        assert!(PerThousand::new(1001).is_none());
        assert!(PerThousand::new(u16::MAX).is_none());

        {
            let valid_value = rng.gen_range(0..=DENOMINATOR);
            assert_eq!(PerThousand::new(valid_value).unwrap().value(), valid_value);
        }

        {
            let invalid_value = rng.gen_range(1001..=u16::MAX);
            assert!(PerThousand::new(invalid_value).is_none());
        }
    }

    #[rstest]
    #[trace]
    #[case(Seed::from_entropy())]
    fn test_encode_decode(#[case] seed: Seed) {
        let mut rng = test_utils::random::make_seedable_rng(seed);

        let encoded_valid = PerThousand::new_from_rng(&mut rng).encode();
        PerThousand::decode(&mut encoded_valid.as_slice()).unwrap();

        let encoded_invalid = rng.gen_range(1001..=u16::MAX).encode();
        PerThousand::decode(&mut encoded_invalid.as_slice()).unwrap_err();

        let mut encoded_1001: &[u8] = b"\xE9\x03";
        PerThousand::decode(&mut encoded_1001).unwrap_err();
    }
}
