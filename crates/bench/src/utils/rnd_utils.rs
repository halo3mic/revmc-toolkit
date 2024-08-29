use rand_chacha::ChaCha8Rng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::ops::Range;
use eyre::Result;


// todo: could be very inefficient not to leave it as iter
pub fn random_sequence<T>(start: T, end: T, size: usize, seed: Option<[u8; 32]>) -> Result<Vec<T>>
where
    T: Copy + PartialOrd,
    Range<T>: Iterator<Item = T>,
    Vec<T>: FromIterator<<Range<T> as Iterator>::Item>,
{
    let mut rng = if let Some(seed) = seed {
        ChaCha8Rng::from_seed(seed)
    } else {
        ChaCha8Rng::from_entropy()
    };

    let range: Vec<T> = (start..end).collect();
    let mut shuffled = range;
    shuffled.shuffle(&mut rng);
    
    Ok(shuffled.into_iter().take(size).collect())
}