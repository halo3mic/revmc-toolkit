use eyre::Result;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::ops::Range;

pub fn random_sequence<T>(start: T, end: T, size: usize, seed: Option<[u8; 32]>) -> Result<Vec<T>>
where
    T: Copy + PartialOrd,
    Range<T>: Iterator<Item = T>,
{
    random_sequence_with_blacklist(start, end, size, seed, vec![])
}

pub fn random_sequence_with_blacklist<T>(
    start: T,
    end: T,
    size: usize,
    seed: Option<[u8; 32]>,
    blacklist: Vec<T>,
) -> Result<Vec<T>>
where
    T: Copy + PartialOrd,
    Range<T>: Iterator<Item = T>,
{
    let mut rng = if let Some(seed) = seed {
        ChaCha8Rng::from_seed(seed)
    } else {
        ChaCha8Rng::from_entropy()
    };

    let mut sampled_elements: Vec<T> = (start..end)
        .filter(|e| !blacklist.contains(e))
        .collect::<Vec<T>>();
    sampled_elements.shuffle(&mut rng);

    let sampled_elements = sampled_elements.into_iter().take(size).collect();

    Ok(sampled_elements)
}
