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

use serialization::{Decode, Encode, Error, Input, Output};

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
pub struct PerThousand(u16);

impl PerThousand {
    pub fn new(value: u16) -> Option<Self> {
        if value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn value(&self) -> u16 {
        self.0
    }
}

impl Encode for PerThousand {
    fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
        dest.write(&self.0.to_be_bytes());
    }
}

impl Decode for PerThousand {
    fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
        let mut num_be_bytes = [0u8; 2];
        input.read(&mut num_be_bytes)?;
        Self::new(u16::from_be_bytes(num_be_bytes)).ok_or(serialization::Error::from(
            "PerThousand deserialization failed",
        ))
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
    fn test_per_thousand(#[case] seed: Seed) {
        let mut rng = test_utils::random::make_seedable_rng(seed);

        assert_eq!(PerThousand::new(0).unwrap().value(), 0);
        assert_eq!(PerThousand::new(1000).unwrap().value(), 1000);

        assert!(PerThousand::new(1001).is_none());
        assert!(PerThousand::new(u16::MAX).is_none());

        {
            let valid_value = rng.gen_range(0..=1000);
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

        let encoded_valid = PerThousand::new(rng.gen_range(0..=1000)).unwrap().encode();
        PerThousand::decode(&mut encoded_valid.as_slice()).unwrap();

        let encoded_invalid = rng.gen_range(1001..=u16::MAX).encode();
        PerThousand::decode(&mut encoded_invalid.as_slice()).unwrap_err();
    }
}
