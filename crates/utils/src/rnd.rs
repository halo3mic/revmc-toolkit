use rand_chacha::ChaCha8Rng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use eyre::Result;
use std::ops::Range;


pub fn random_sequence<T>(start: T, end: T, size: usize, seed: Option<[u8; 32]>) -> Result<Vec<T>>
where
    T: Copy + PartialOrd,
    Range<T>: Iterator<Item = T>,
{
    let mut rng = if let Some(seed) = seed {
        ChaCha8Rng::from_seed(seed)
    } else {
        ChaCha8Rng::from_entropy()
    };
    
    let mut sampled_elements: Vec<T> = (start..end).take(size).collect();    
    sampled_elements.shuffle(&mut rng);

    Ok(sampled_elements)
}
