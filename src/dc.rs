// Experimental DC removal code that is transparent to
// bit errors and is not causing extra damage to the underlying
// data in the presence of noise.
//
// The idea is to make this a fixed bit position stuffing code
// to make the decoding algorithm independent on what corruption
// hapened to the data during transmission.
//
// This is an instance of the 6b -> 8b code https://en.wikipedia.org/wiki/6b/8b_encoding
// and maintains the same or better guarantees - no more than 6 consecutive symbols ever

pub const fn balance(raw: u8) -> u8 {
    // a b X c d Y e f
    let ones_left = (raw >> 2).count_ones();
    let ones_right = (raw & 0xf).count_ones();

    let b_x = if ones_left > 2 { 0 } else { 1 };

    let b_y = if ones_right < 2 { 1 } else { 0 };

    ((raw >> 4) & 0x3) << 6 | b_x << 5 | ((raw >> 2) & 0x3) << 3 | b_y << 2 | raw & 0x3
}

pub const fn strip(enc: u8) -> u8 {
    (enc & 0b11000000) >> 2 | (enc & 0b00011000) >> 1 | (enc & 0b00000011)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_full_reversability() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = balance(b);
            let decoded = strip(encoded);
            assert_eq!(
                b, decoded,
                "6 to 8 reversability broken for 0x{b:x} (encoded 0x{encoded:x}, decoded 0x{decoded:x})",
            );
        }
    }

    // Count the longest bit sequence in the lowest `len` bits of `code`
    fn longest_bit_sequence(code: u16, len: usize) -> u8 {
        let mut last = None;
        let mut count = 0;
        let mut max_count = 0;

        for i in 0..len {
            let bit = (code >> i) & 0x1;
            if last.is_none() || last.unwrap() != bit {
                count = 0;
            }

            last = Some(bit);
            count += 1;
            max_count = max_count.max(count);
        }

        max_count
    }

    #[test]
    fn test_max_sequence_in_isolation() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = balance(b);
            let sequence = longest_bit_sequence(encoded as u16, 8);

            assert!(
                sequence <= 3,
                "6 to 8 contains long streak of {sequence} same bits for 0x{b:x} (encoded 0x{encoded:x})",
            );
        }
    }

    #[test]
    fn test_max_sequence_in_sequence() {
        // Test each two 6b symbols
        for b1 in 0_u8..=0x3f {
            for b2 in 0_u8..=0x3f {
                let encoded1 = balance(b1);
                let encoded2 = balance(b2);

                let sequence = longest_bit_sequence((encoded1 as u16) << 8 | encoded2 as u16, 16);

                assert!(sequence <= 5, "6 to 8 contains long streak of {sequence} same bits for 0x{b1:x}|{b2:x} (encoded 0x{encoded1:x}|{encoded2:x})");
            }
        }
    }

    #[test]
    fn test_avg_sequence_in_sequence() {
        // Test each two 6b symbols
        let mut sequence = 0_u32;
        for b1 in 0_u8..=0x3f {
            for b2 in 0_u8..=0x3f {
                let encoded1 = balance(b1);
                let encoded2 = balance(b2);

                sequence +=
                    longest_bit_sequence((encoded1 as u16) << 8 | encoded2 as u16, 16) as u32;
            }
        }

        sequence *= 1000;
        sequence /= 64 * 64;

        assert!(
            sequence < 3000,
            "Average sequence length is {sequence} / 1000"
        );
    }

    #[test]
    fn test_bit_error_impact() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = balance(b);
            for i in 0..8 {
                let xor = 1_u8 << i;
                let decoded = strip(encoded ^ xor);
                let error_bits = b ^ decoded;

                assert!(
                    error_bits.count_ones() <= 1,
                    "6 to 8 reverse broken with {} bit errors in 0x{:x} and bitflip mask 0x{:x}",
                    error_bits.count_ones(),
                    b,
                    xor
                );
            }
        }
    }

    #[test]
    fn test_two_bit_error_impact() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = balance(b);
            for i in 0..8 {
                for j in 0..8 {
                    if i == j {
                        continue;
                    }

                    let xor = 1_u8 << i | 1_u8 << j;
                    let decoded = strip(encoded ^ xor);
                    let error_bits = b ^ decoded;

                    assert!(error_bits.count_ones() <= 2, "6 to 8 reverse broken with {} bit errors in 0x{:x} and bitflip mask 0x{:x}", error_bits.count_ones(), b, xor);
                }
            }
        }
    }
}
