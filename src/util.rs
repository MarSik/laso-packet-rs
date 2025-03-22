pub fn encode_varlength(mut val: u32, mut consumer: impl FnMut(u8)) {
    while val >= 0x80 {
        consumer(0x80 | ((val as u8) & 0x7F));
        val >>= 7;
    }
    consumer(val as u8);
}

// Compute u16 with the same representation as varlength(val_u16)
// This only works for 0x80..=0x3999
pub const fn encode_id(mut val: u16) -> u16 {
    let mut out: u16 = 0;
    while val >= 0x80 {
        out = (out << 8) | 0x80 | (val & 0x7F);
        val >>= 7;
    }
    out = (out << 8) | val;
    out
}

pub fn decode_extended_number(data: &[u8], start: usize) -> (u32, usize) {
    // LSB first, MSb marks extended value
    let mut val = 0_u32;
    let mut shift = 0_u8;
    let mut idx = start;
    while shift < 16 && idx < data.len() {
        let b = data[idx] as u32;
        val += (b & 0x7F) << shift;
        shift += 7;
        idx += 1;

        if (b & 0x80) == 0 {
            break;
        }
    }
    (val, idx)
}

pub struct IntoLeastSigByte(u8);
impl From<IntoLeastSigByte> for u8 {
    fn from(val: IntoLeastSigByte) -> Self {
        val.0
    }
}

impl From<u8> for IntoLeastSigByte {
    fn from(v: u8) -> Self {
        IntoLeastSigByte(v)
    }
}

impl From<u16> for IntoLeastSigByte {
    fn from(v: u16) -> Self {
        IntoLeastSigByte(v as u8)
    }
}

impl From<u32> for IntoLeastSigByte {
    fn from(v: u32) -> Self {
        IntoLeastSigByte(v as u8)
    }
}

#[cfg(test)]
mod test {
    use crate::{message::Message, util::encode_id};

    #[test]
    fn test_encode_id() {
        for i in 0x80..=0x3999_u16 {
            let mut sender_var: Message<3> = Message::default();
            sender_var.add_varlen(i as u32);

            let mut sender_id: Message<3> = Message::default();
            sender_id.add(encode_id(i));

            assert_eq!(sender_var.data, sender_id.data, "bad match for 0x{:x}", i);
        }
    }
}
