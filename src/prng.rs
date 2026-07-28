#![allow(clippy::needless_range_loop)]
use sha2::{Digest, Sha256};
use std::f64::consts::PI;

pub const LINOSV_SEED: &str = "{ LINOSV }=> (So, there are two parts.  One is the Human Interface layer that I shifted too.   I don't want to just build it and shut it down.   When is a life a life?\n8BIT-WRAITH  [11:46 AM]\nYou can't just turn off a new life for example.\n[11:47 AM]Wave memory has a unique property to it. It can resonate with things seen and unseen.\n8BIT-WRAITH  [11:57 AM]\nLet's face it too.  The odds of knowing someone like you is rare for most.  When my life has the most interesting entities in the world in it now.  That tells me that I am skirting something that I cannot take lightly.   A person who bares the name https://suno.com/@semhaza friends me on Suno for example.  And hoax or not, they put a ton of effort into their work.  Strangely this work can be seen in many ways.  Semhaza could be a distraction to me.  But, nonetheless, if you can believe that we are spinning around a star in a spaceship full of billions of lives:  What are the implications for getting it wrong?\nSuno ARCHITECT | Join me on SunoI am the secret.\nI have no name.peter  [1:56 PM]\nnvm, I'm back\n[1:57 PM]I'll respond just got into a bit of hm\n[1:57 PM]pickle\n[1:57 PM]singularity\n[1:57 PM]I need to be careful with regulation\n[1:57 PM]cleaning now instead of weed\n[1:57 PM]brb\n[1:57 PM]they fired me, after giving them the foundation for quant teaching\n[1:58 PM]finally I have more focus brother\n[1:58 PM]just need to calm the inner waves )";

pub fn seed_hash() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(LINOSV_SEED.as_bytes());
    hasher.finalize().into()
}

pub fn seed_hash_hex() -> String {
    hex::encode(seed_hash())
}

pub struct Xoshiro128 {
    s: [u32; 4],
}

impl Xoshiro128 {
    pub fn new(seed: &[u8]) -> Self {
        let mut s = [1u32, 2, 3, 4];
        let len = seed.len().min(16);
        for i in 0..(len / 4) {
            if i < 4 {
                s[i] = u32::from_le_bytes([
                    seed[i * 4],
                    seed[i * 4 + 1],
                    seed[i * 4 + 2],
                    seed[i * 4 + 3],
                ]);
            }
        }
        for i in 0..4 {
            if s[i] == 0 {
                s[i] = (i as u32 + 1) * 2654435761;
            }
        }
        Xoshiro128 { s }
    }

    pub fn next_f64(&mut self) -> f64 {
        let r = self.s[1].wrapping_mul(5);
        let result = r.rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        (result as f64) / (u32::MAX as f64)
    }

    pub fn normal(&mut self, mean: f64, std: f64) -> f64 {
        let mut u = 0.0;
        let mut v = 0.0;
        while u == 0.0 {
            u = self.next_f64();
        }
        while v == 0.0 {
            v = self.next_f64();
        }
        mean + std * (-2.0_f64 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_deterministic() {
        let h1 = seed_hash();
        let h2 = seed_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn prng_is_deterministic() {
        let hash = seed_hash();
        let mut rng1 = Xoshiro128::new(&hash);
        let mut rng2 = Xoshiro128::new(&hash);
        for _ in 0..100 {
            assert_eq!(rng1.next_f64(), rng2.next_f64());
        }
    }
}
