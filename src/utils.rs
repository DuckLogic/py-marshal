use num_bigint::{BigUint, Sign};
use num_traits::Zero;
use std::cmp::Ordering;

/// Based on `_PyLong_AsByteArray` in <https://github.com/python/cpython/blob/master/Objects/longobject.c>
#[allow(clippy::cast_possible_truncation)]
pub fn biguint_from_pylong_digits(digits: &[u16]) -> BigUint {
    if digits.is_empty() {
        return BigUint::zero();
    };
    assert!(digits[digits.len() - 1] != 0);
    let mut accum: u64 = 0;
    let mut accumbits: u8 = 0;
    let mut p = Vec::<u32>::new();
    for (i, &thisdigit) in digits.iter().enumerate() {
        accum |= u64::from(thisdigit) << accumbits;
        accumbits += if i == digits.len() - 1 {
            16 - (thisdigit.leading_zeros() as u8)
        } else {
            15
        };

        // Modified to get u32s instead of u8s.
        while accumbits >= 32 {
            p.push(accum as u32);
            accumbits -= 32;
            accum >>= 32;
        }
    }
    assert!(accumbits < 32);
    if accumbits > 0 {
        p.push(accum as u32);
    }
    BigUint::new(p)
}

pub fn sign_of<T: Ord + Zero>(x: &T) -> Sign {
    match x.cmp(&T::zero()) {
        Ordering::Less => Sign::Minus,
        Ordering::Equal => Sign::NoSign,
        Ordering::Greater => Sign::Plus,
    }
}

#[cfg(test)]
mod test {
    use super::biguint_from_pylong_digits;
    use num_bigint::BigUint;

    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn test_biguint_from_pylong_digits() {
        assert_eq!(
            biguint_from_pylong_digits(&[
                0b000_1101_1100_0100,
                0b110_1101_0010_0100,
                0b001_0000_1001_1101
            ]),
            BigUint::from(0b001_0000_1001_1101_110_1101_0010_0100_000_1101_1100_0100_u64)
        );
    }
}
