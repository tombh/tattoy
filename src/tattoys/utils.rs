//! Useful common code

use rand::Rng;

/// Given a number, roll a dice of that size, and if it rolls a 1 then return `true`
#[must_use]
pub fn is_random_trigger(chance: i64) -> bool {
    let rng = rand::thread_rng().gen_range(1_i64..=chance);
    rng == 1_i64
}
