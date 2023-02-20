use super::*;
use crate::primitives::id::{default_hash, DefaultHashAlgoStream};
use crypto::hash::StreamHasher;
use rstest::rstest;
use test_utils::random::{make_seedable_rng, Rng, Seed};

#[test]
fn merkletree_too_small() {
    let t0 = MerkleTree::from_leaves(vec![]);
    assert_eq!(t0.unwrap_err(), MerkleTreeFormError::TooSmall(0));
}

#[test]
fn merkletree_basic_two_leaf_node() {
    let v1 = default_hash(H256::zero());
    let v2 = default_hash(H256::from_low_u64_be(1));

    let t = MerkleTree::from_leaves(vec![v1, v2]).unwrap();

    // recreate the expected root
    let mut test_hasher = DefaultHashAlgoStream::new();
    test_hasher.write(v1);
    test_hasher.write(v2);

    assert_eq!(t.root(), test_hasher.finalize().into());
}

#[test]
fn merkletree_basic_four_leaf_node() {
    let v1 = default_hash(H256::zero());
    let v2 = default_hash(H256::from_low_u64_be(1));
    let v3 = default_hash(H256::from_low_u64_be(2));
    let v4 = default_hash(H256::from_low_u64_be(3));

    let t = MerkleTree::from_leaves(vec![v1, v2, v3, v4]).unwrap();

    // recreate the expected root
    let mut node10 = DefaultHashAlgoStream::new();
    node10.write(v1);
    node10.write(v2);

    let mut node11 = DefaultHashAlgoStream::new();
    node11.write(v3);
    node11.write(v4);

    let mut node00 = DefaultHashAlgoStream::new();
    let n10 = node10.finalize();
    node00.write(n10);
    let n11 = node11.finalize();
    node00.write(n11);

    let res = node00.finalize();

    assert_eq!(t.root(), res.into());
}

#[test]
fn merkletree_basic_eight_leaf_node() {
    let v1 = default_hash(H256::zero());
    let v2 = default_hash(H256::from_low_u64_be(1));
    let v3 = default_hash(H256::from_low_u64_be(2));
    let v4 = default_hash(H256::from_low_u64_be(3));
    let v5 = default_hash(H256::from_low_u64_be(4));
    let v6 = default_hash(H256::from_low_u64_be(5));
    let v7 = default_hash(H256::from_low_u64_be(6));
    let v8 = default_hash(H256::from_low_u64_be(7));

    let t = MerkleTree::from_leaves(vec![v1, v2, v3, v4, v5, v6, v7, v8]).unwrap();

    // recreate the expected root
    let mut node20 = DefaultHashAlgoStream::new();
    node20.write(v1);
    node20.write(v2);

    let mut node21 = DefaultHashAlgoStream::new();
    node21.write(v3);
    node21.write(v4);

    let mut node22 = DefaultHashAlgoStream::new();
    node22.write(v5);
    node22.write(v6);

    let mut node23 = DefaultHashAlgoStream::new();
    node23.write(v7);
    node23.write(v8);

    let n20 = node20.finalize();
    let n21 = node21.finalize();
    let n22 = node22.finalize();
    let n23 = node23.finalize();

    let mut node10 = DefaultHashAlgoStream::new();
    node10.write(n20);
    node10.write(n21);

    let mut node11 = DefaultHashAlgoStream::new();
    node11.write(n22);
    node11.write(n23);

    let n10 = node10.finalize();
    let n11 = node11.finalize();

    let mut node00 = DefaultHashAlgoStream::new();
    node00.write(H256::from(n10));
    node00.write(H256::from(n11));

    let res = node00.finalize();

    assert_eq!(t.root(), H256::from(res));
}

#[test]
fn merkletree_with_arbitrary_length_2() {
    let v1 = H256::zero();
    let v2 = H256::from_low_u64_be(1);

    let t = MerkleTree::from_leaves(vec![v1, v2]).unwrap();

    // recreate the expected root
    let mut test_hasher = DefaultHashAlgoStream::new();
    test_hasher.write(v1);
    test_hasher.write(v2);

    assert_eq!(t.root(), test_hasher.finalize().into());
}

#[test]
fn merkletree_with_arbitrary_length_3() {
    let v1 = H256::zero();
    let v2 = H256::from_low_u64_be(1);
    let v3 = H256::from_low_u64_be(2);

    let t = MerkleTree::from_leaves(vec![v1, v2, v3]).unwrap();

    // recreate the expected root
    let mut node10 = DefaultHashAlgoStream::new();
    node10.write(v1);
    node10.write(v2);

    let mut node11 = DefaultHashAlgoStream::new();
    node11.write(v3);
    node11.write(default_hash(v3));

    let mut node00 = DefaultHashAlgoStream::new();
    let n10 = node10.finalize();
    node00.write(n10);
    let n11 = node11.finalize();
    node00.write(n11);

    let res = node00.finalize();

    assert_eq!(t.root(), res.into());
}

#[test]
fn merkletree_with_arbitrary_length_5() {
    let v1 = H256::zero();
    let v2 = H256::from_low_u64_be(1);
    let v3 = H256::from_low_u64_be(2);
    let v4 = H256::from_low_u64_be(3);
    let v5 = H256::from_low_u64_be(4);
    let v6 = default_hash(v5);
    let v7 = default_hash(v6);
    let v8 = default_hash(v7);

    let t = MerkleTree::from_leaves(vec![v1, v2, v3, v4, v5]).unwrap();

    // recreate the expected root
    let mut node20 = DefaultHashAlgoStream::new();
    node20.write(v1);
    node20.write(v2);

    let mut node21 = DefaultHashAlgoStream::new();
    node21.write(v3);
    node21.write(v4);

    let mut node22 = DefaultHashAlgoStream::new();
    node22.write(v5);
    node22.write(v6);

    let mut node23 = DefaultHashAlgoStream::new();
    node23.write(v7);
    node23.write(v8);

    let n20 = node20.finalize();
    let n21 = node21.finalize();
    let n22 = node22.finalize();
    let n23 = node23.finalize();

    let mut node10 = DefaultHashAlgoStream::new();
    node10.write(n20);
    node10.write(n21);

    let mut node11 = DefaultHashAlgoStream::new();
    node11.write(n22);
    node11.write(n23);

    let n10 = node10.finalize();
    let n11 = node11.finalize();

    let mut node00 = DefaultHashAlgoStream::new();
    node00.write(n10);
    node00.write(n11);

    let res = node00.finalize();

    assert_eq!(t.root(), res.into());
}

#[test]
fn leaves_count_from_tree_size() {
    for i in 1..30 {
        let leaves_count = 1 << (i - 1);
        let tree_size = (1 << i) - 1;
        assert_eq!(
            MerkleTree::leaves_count_from_tree_size(tree_size.try_into().unwrap()),
            NonZeroUsize::new(leaves_count).unwrap(),
            "Check failed for i = {}",
            i
        );
    }
}

#[rstest]
#[should_panic(expected = "A valid tree size is always a power of 2 minus one")]
#[case(Seed::from_entropy())]
fn leaves_count_from_tree_size_error(#[case] seed: Seed) {
    let mut rng = make_seedable_rng(seed);
    let mut i = rng.gen::<usize>();
    while (i + 1usize).count_ones() == 1 {
        i = rng.gen::<usize>();
    }
    let _leaves_count = MerkleTree::leaves_count_from_tree_size(i.try_into().unwrap());
}

#[test]
fn bottom_access_one_leaf() {
    let v00 = H256::from_low_u64_be(1);

    let t = MerkleTree::from_leaves(vec![v00]).unwrap();

    assert_eq!(t.node_from_bottom(0, 0).unwrap(), v00);
}

#[test]
fn bottom_access_two_leaves() {
    let v00 = H256::zero();
    let v01 = H256::from_low_u64_be(1);

    let t = MerkleTree::from_leaves(vec![v00, v01]).unwrap();

    assert_eq!(t.node_from_bottom(0, 0).unwrap(), v00);
    assert_eq!(t.node_from_bottom(0, 1).unwrap(), v01);

    let v10 = MerkleTree::combine_pair(&v00, &v01);

    assert_eq!(t.node_from_bottom(1, 0).unwrap(), v10);
}

#[test]
fn bottom_access_four_leaves() {
    let v00 = H256::zero();
    let v01 = H256::from_low_u64_be(1);
    let v02 = H256::from_low_u64_be(2);
    let v03 = H256::from_low_u64_be(3);

    let t = MerkleTree::from_leaves(vec![v00, v01, v02, v03]).unwrap();

    assert_eq!(t.node_from_bottom(0, 0).unwrap(), v00);
    assert_eq!(t.node_from_bottom(0, 1).unwrap(), v01);
    assert_eq!(t.node_from_bottom(0, 2).unwrap(), v02);
    assert_eq!(t.node_from_bottom(0, 3).unwrap(), v03);

    let v10 = MerkleTree::combine_pair(&v00, &v01);
    let v11 = MerkleTree::combine_pair(&v02, &v03);

    assert_eq!(t.node_from_bottom(1, 0).unwrap(), v10);
    assert_eq!(t.node_from_bottom(1, 1).unwrap(), v11);

    let v20 = MerkleTree::combine_pair(&v10, &v11);

    assert_eq!(t.node_from_bottom(2, 0).unwrap(), v20);
}

#[test]
fn bottom_access_eight_leaves() {
    let v00 = H256::zero();
    let v01 = H256::from_low_u64_be(1);
    let v02 = H256::from_low_u64_be(2);
    let v03 = H256::from_low_u64_be(3);
    let v04 = H256::from_low_u64_be(4);
    let v05 = default_hash(v04);
    let v06 = default_hash(v05);
    let v07 = default_hash(v06);

    let t = MerkleTree::from_leaves(vec![v00, v01, v02, v03, v04]).unwrap();

    assert_eq!(t.node_from_bottom(0, 0).unwrap(), v00);
    assert_eq!(t.node_from_bottom(0, 1).unwrap(), v01);
    assert_eq!(t.node_from_bottom(0, 2).unwrap(), v02);
    assert_eq!(t.node_from_bottom(0, 3).unwrap(), v03);
    assert_eq!(t.node_from_bottom(0, 4).unwrap(), v04);
    assert_eq!(t.node_from_bottom(0, 5).unwrap(), v05);
    assert_eq!(t.node_from_bottom(0, 6).unwrap(), v06);
    assert_eq!(t.node_from_bottom(0, 7).unwrap(), v07);

    let v10 = MerkleTree::combine_pair(&v00, &v01);
    let v11 = MerkleTree::combine_pair(&v02, &v03);
    let v12 = MerkleTree::combine_pair(&v04, &v05);
    let v13 = MerkleTree::combine_pair(&v06, &v07);

    assert_eq!(t.node_from_bottom(1, 0).unwrap(), v10);
    assert_eq!(t.node_from_bottom(1, 1).unwrap(), v11);
    assert_eq!(t.node_from_bottom(1, 2).unwrap(), v12);
    assert_eq!(t.node_from_bottom(1, 3).unwrap(), v13);

    let v20 = MerkleTree::combine_pair(&v10, &v11);
    let v21 = MerkleTree::combine_pair(&v12, &v13);

    assert_eq!(t.node_from_bottom(2, 0).unwrap(), v20);
    assert_eq!(t.node_from_bottom(2, 1).unwrap(), v21);

    let v30 = MerkleTree::combine_pair(&v20, &v21);
    assert_eq!(t.node_from_bottom(3, 0).unwrap(), v30);
}

#[test]
fn position_from_index_1_tree_element() {
    let tree_size: NonZeroUsize = 1.try_into().unwrap();
    {
        let level = 0;
        let level_start = 0;
        let level_end: usize = 1;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
}

#[test]
fn position_from_index_3_tree_elements() {
    let tree_size: NonZeroUsize = 3.try_into().unwrap();
    {
        let level = 0;
        let level_start = 0;
        let level_end: usize = 2;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 1;
        let level_start = 2;
        let level_end: usize = 3;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
}

#[test]
fn position_from_index_7_tree_elements() {
    let tree_size: NonZeroUsize = 7.try_into().unwrap();
    {
        let level = 0;
        let level_start = 0;
        let level_end: usize = 4;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 1;
        let level_start = 4;
        let level_end: usize = 6;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 2;
        let level_start = 6;
        let level_end: usize = 7;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
}

#[test]
fn position_from_index_15_tree_elements() {
    let tree_size: NonZeroUsize = 15.try_into().unwrap();
    {
        let level = 0;
        let level_start = 0;
        let level_end: usize = 8;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 1;
        let level_start = 8;
        let level_end: usize = 12;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 2;
        let level_start = 12;
        let level_end: usize = 14;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
    {
        let level = 3;
        let level_start = 14;
        let level_end: usize = 15;
        for i in level_start..level_end {
            assert_eq!(
                MerkleTree::position_from_index(tree_size, i),
                (level, i - level_start)
            );
        }
    }
}
