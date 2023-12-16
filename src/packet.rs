use bitvec::macros::internal::funty::Fundamental;
use core::mem::size_of;
use core::ops::Shr;
use heapless::spsc::Queue;
use heapless::Vec;
use ignore_result::Ignore;

#[derive(Clone)]
pub struct RawReceiveData<const N: usize> {
    pub packet: Vec<u8, N>,
    pub lna: u8,
    pub rssi: u8,
}

impl <const N: usize> RawReceiveData<N> {
    pub fn init() -> Self {
        Self {
            packet: Vec::new(),
            lna: 0,
            rssi: 0,
        }
    }

    pub fn clear(&mut self) {
        self.packet.clear();
    }
}

#[repr(u16)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum PacketType {
    Unknown = 0x00,
    // TODO
    Temperature = 0x101,
    WaterLevel = 0x10A,
    GsmStatus = 0x102,
}

pub fn encode_varlength(mut val: u32, mut consumer: impl FnMut(u8)) {
    while val >= 0x80 {
        consumer(0x80 | (val.as_u8() & 0x7F));
        val >>= 7;
    }
    consumer(val as u8);
}

impl PacketType {
    pub fn encode(self, consumer: impl FnMut(u8)) {
        let val = self as u16;
        encode_varlength(val.as_u32(), consumer);
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RxMessage<const N: usize> {
    pub data: Vec<u8, { N }>,
    pub ack_wanted: bool,
    pub ack: bool,
    pub source_address: u16,
    pub packet_type: u16,
    pub header_length: u8,
    pub rssi: u8,
    pub lna: u8,
    pub errors: u8,
}

impl<const N: usize> RxMessage<{ N }> {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            ack_wanted: false,
            ack: false,
            source_address: 0,
            packet_type: 0,
            header_length: 0,
            rssi: 0,
            lna: 0,
            errors: 0,
        }
    }

    pub fn parse(&mut self) {
        let mut skip = 0;
        (self.packet_type, skip) = decode_extended_number(&self.data, 0);
        (self.source_address, skip) = decode_extended_number(&self.data, skip);
        self.header_length = skip as u8;
    }
}

impl<const N: usize> From<Message<N>> for RxMessage<N> {
    fn from(m: Message<N>) -> Self {
        let mut rx = RxMessage::new();
        rx.packet_type = m.packet_type as u16;
        rx.source_address = m.source_address;
        for b in m.data.iter() {
            rx.data.push(*b).ignore();
        }
        rx
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Message<const N: usize> {
    pub data: Queue<u8, { N }>,
    pub ack_wanted: bool,
    pub ack: bool,
    pub source_address: u16,
    pub packet_type: PacketType,
}

pub struct IntoLeastSigByte(u8);
impl Into<u8> for IntoLeastSigByte {
    fn into(self) -> u8 {
        self.0
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

impl<const N: usize> Message<N> {
    pub fn new() -> Self {
        Self {
            data: Queue::new(),
            ack_wanted: false,
            ack: false,
            source_address: 0,
            packet_type: PacketType::Unknown,
        }
    }

    pub fn sender(self) -> MessageSender<{ N }> {
        MessageSender {
            message: self,
            first: true,
        }
    }

    pub fn add<T: Shr<usize, Output = T> + Into<IntoLeastSigByte> + Copy>(&mut self, v: T) {
        let mut bits = size_of::<T>() * 8;
        while bits >= 8 {
            bits -= 8;
            let bw = (v >> bits).into();
            let b8 = bw.into();
            self.data.enqueue(b8);
        }
    }

    pub fn add_varlen(&mut self, v: u32) {
        encode_varlength(v, |b| {
            self.data.enqueue(b);
        });
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct MessageSender<const N: usize> {
    message: Message<{ N }>,
    first: bool,
}

impl<const N: usize> MessageSender<N> {
    pub fn data_to_send(&self) -> bool {
        !self.message.data.is_empty()
    }

    pub fn packet(&mut self) -> PacketData {
        let mut p = PacketData::new();

        if self.first {
            // Queue source address and packet type
            // First packet always has enough space for this
            self.message.packet_type.encode(|b| {
                p.data.push(b).ignore();
            });
            encode_varlength(self.message.source_address.as_u32(), |b| {
                p.data.push(b).ignore();
            });
        }

        while !p.data.is_full() && !self.message.data.is_empty() {
            p.data.push(self.message.data.dequeue().unwrap()).unwrap();
        }

        while !p.data.is_full() {
            p.data.push(0u8).unwrap();
        }

        p.last = self.message.data.is_empty();
        p.first = self.first;
        self.first = false;

        return p;
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PacketData {
    pub data: Vec<u8, 11>,
    pub first: bool,
    pub last: bool,
}

impl PacketData {
    pub fn new() -> PacketData {
        return PacketData {
            data: Vec::new(),
            first: false,
            last: false,
        };
    }

    fn crc(acc: u8, v: &u8) -> u8 {
        acc.overflowing_add(*v).0
    }

    fn compute_cfg_byte(&self) -> u8 {
        let mut crc8: u8 = self.data.iter().fold(0x55u8, Self::crc);

        let mut flags: u8 = 0;
        if self.first {
            flags += 0x4;
        }
        if !self.last {
            flags += 0x1;
        }

        crc8 = crc8.overflowing_add(flags).0;

        let ucrc = crc8 >> 4;
        let lcrc = crc8 & 0xf;
        let crc4 = ucrc.overflowing_add(lcrc).0;

        let ret: u8 = flags | (crc4 << 4);

        return ret;
    }

    pub fn to_wire_data(&self) -> [u8; 12] {
        let mut out = [0u8; 12];
        for (idx, v) in self.data.iter().enumerate() {
            out[idx] = *v;
        }
        out[11] = self.compute_cfg_byte();
        return out;
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PacketWithGolay {
    data: [u8; 24],
}

impl PacketWithGolay {
    // http://aqdi.com/articles/using-the-golay-error-detection-and-correction-code-3/

    //fn apply_golay(raw : u16) -> u32 {
    //    golay::encode(raw)
    //}

    pub fn new() -> Self {
        Self { data: [0; 24] }
    }

    const POLY: u32 = 0xAE3;

    fn apply_golay(c: u16) -> u32 {
        debug_assert_eq!(c >> 12, 0);

        let s = Self::syndrome(c.into());
        let code = s.as_u32() | c.as_u32();

        return (Self::parity_24b(code) << 23) | code; /* assemble codeword */
    }

    fn syndrome(mut cw: u32) -> u32 {
        /* This function calculates and returns the syndrome
        of a [23,12] Golay codeword. */
        cw &= 0x7fffff_u32;

        for _i in 1..=12 {
            /* examine each data bit */
            if (cw & 1) > 0 {
                /* test data bit */
                cw ^= Self::POLY; /* XOR polynomial */
            }
            cw >>= 1; /* shift intermediate result */
        }

        return cw << 12; /* value pairs with upper bits of cw */
    }

    fn parity_24b(cw: u32) -> u32 {
        let mut parity = (cw >> 16) as u8;
        parity ^= (cw >> 8) as u8;
        parity ^= (cw >> 0) as u8;

        parity = (parity >> 4) ^ parity;
        parity = (parity >> 2) ^ parity;
        parity = (parity >> 1) ^ parity;
        parity.as_u32() & 0x1_u32
    }

    fn count_ones(mut b: u32) -> usize {
        const ONES: [u8; 16] = [0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4];
        let mut sum: usize = 0;
        while b > 0 {
            sum += ONES[(b & 0xf) as usize] as usize;
            b = b >> 4;
        }
        sum
    }

    /* This function rotates 23 bit codeword cw left by n bits. */
    fn rotate_left(mut cw: u32, n: usize) -> u32 {
        for _i in 1..=n {
            if (cw & 0x400000) != 0 {
                cw = (cw << 1) | 1;
            } else {
                cw <<= 1;
            }
        }

        return cw & 0x7fffff;
    }

    /* This function rotates 23 bit codeword cw right by n bits. */
    fn rotate_right(mut cw: u32, n: usize) -> u32 {
        for _i in 1..=n {
            if (cw & 1) != 0 {
                cw = (cw >> 1) | 0x400000;
            } else {
                cw >>= 1;
            }
        }

        return cw & 0x7fffff;
    }

    fn undo_golay(raw: u32) -> (u16, usize, bool) {
        //golay::decode(raw).unwrap_or((0_u16, 12))
        let mut mask: u32 = 0x1; /* mask for bit flipping, start with Lsb */

        let cwsaver = raw; /* saves initial value of cw */
        let mut cw = raw;

        let mut w = 3; /* current syndrome limit weight, 2 or 3, initial syndrome weight threshold = 3 */
        let mut j = -1; /* -1 = no trial bit flipping on first pass */

        while j < 23
        /* flip each trial bit */
        {
            if j != -1
            /* toggle a trial bit */
            {
                if j > 0
                /* restore last trial bit */
                {
                    cw = cw ^ mask; /* xor with old mask */
                    mask <<= 1; /* point to next bit */
                }
                cw = cw ^ mask; /* flip next trial bit by xoring with mask */
                w = 2; /* lower the threshold while bit diddling as another error might have been introduced */
            }

            let mut s = Self::syndrome(cw); /* look for errors */

            if s != 0
            /* errors exist */
            {
                for i in 0..23
                /* check syndrome of each cyclic shift */
                {
                    let weight = Self::count_ones(s);
                    if weight <= w
                    /* syndrome matches error pattern */
                    {
                        cw = cw ^ s; /* remove errors by xoring with syndrome */
                        cw = Self::rotate_right(cw, i); /* unrotate data */

                        let c = (cw & 0xfff) as u16;
                        if j >= 0 {
                            /* count toggled bit (per Steve Duncan) */
                            return (c, weight + 1, Self::parity_24b(cw) == 0);
                        } else {
                            return (c, weight, Self::parity_24b(cw) == 0);
                        }
                    } else {
                        cw = Self::rotate_left(cw, 1); /* rotate to next pattern */
                        s = Self::syndrome(cw); /* calc new syndrome */
                    }
                }

                j += 1; /* toggle next trial bit */
            } else {
                return ((cw & 0xfff) as u16, 0, Self::parity_24b(cw) == 0); /* return corrected codeword */
            }
        }

        return ((cwsaver & 0xfff) as u16, 0, Self::parity_24b(cwsaver) == 0); /* return original if no corrections */
    }

    pub fn from(p: &PacketData) -> Self {
        let mut ret = PacketWithGolay { data: [0u8; 24] };

        let wire = p.to_wire_data();

        let mut i_src = 0;
        let mut i_dst = 0;

        while i_src < wire.len() {
            let src1 = ((wire[i_src] as u16) << 4) + ((wire[i_src + 1] >> 4) as u16);
            let src2 = (((wire[i_src + 1] as u16) << 8) + (wire[i_src + 2] as u16)) & 0xfff;

            let dst1 = Self::apply_golay(src1);
            let dst2 = Self::apply_golay(src2);

            ret.data[i_dst] = (dst1 >> 16) as u8;
            ret.data[i_dst + 1] = (dst1 >> 8) as u8;
            ret.data[i_dst + 2] = (dst1 >> 0) as u8;

            ret.data[i_dst + 3] = (dst2 >> 16) as u8;
            ret.data[i_dst + 4] = (dst2 >> 8) as u8;
            ret.data[i_dst + 5] = (dst2 >> 0) as u8;

            i_src += 3;
            i_dst += 6;
        }

        return ret;
    }

    pub fn to(&self, p: &mut PacketData, errtotal: &mut usize) -> bool {
        let mut buff = [0_u8; 12];

        let mut i_src = 0;
        let mut i_dst = 0;

        while i_src < self.data.len() {
            let src1 = ((self.data[i_src] as u32) << 16)
                + ((self.data[i_src + 1] as u32) << 8)
                + (self.data[i_src + 2] as u32);
            let src2 = ((self.data[i_src + 3] as u32) << 16)
                + ((self.data[i_src + 4] as u32) << 8)
                + (self.data[i_src + 5] as u32);

            let (dst1, err1, _parity1) = Self::undo_golay(src1);
            let (dst2, err2, _parity2) = Self::undo_golay(src2);

            buff[i_dst] = (dst1 >> 4) as u8; // [12:4]
            buff[i_dst + 1] = (((dst1 & 0xf) << 4) as u8) + (((dst2 & 0xf00) >> 8) as u8); // [4:0] [12:8]
            buff[i_dst + 2] = dst2 as u8; // [8:0]

            *errtotal += err1 + err2;

            i_src += 6;
            i_dst += 3;
        }

        p.data.clear();
        for i in 0..11 {
            // The destination is sized properly to take 11B
            p.data.push(buff[i]).ignore();
        }

        // Compute flags and compare CRC
        let flags = buff[11];
        p.first = (flags & 0x4) > 0;
        p.last = (flags & 0x1) == 0;

        return p.compute_cfg_byte() == flags;
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PacketWithInterleave {
    data: [u8; 24],
}

impl PacketWithInterleave {
    pub fn new() -> Self {
        Self { data: [0; 24] }
    }

    #[inline(always)]
    fn g3B(msb: u8, isb: u8, lsb: u8) -> u32 {
        ((msb as u32) << 16) + ((isb as u32) << 8) + (lsb as u32)
    }

    fn g1b(w: u32, sh: usize) -> u8 {
        ((w & 0x1) as u8) << sh
    }

    // Transform 8 24b src chunks into 24 8b dst chunks
    // [A23 .. A0][B23 .. B0] ... [H23 .. H0] -> [A0 B0 .. H0][A1 B1 .. H1] ... [A23 B23 .. H23]
    pub fn from(p: &PacketWithGolay) -> Self {
        let mut ret = PacketWithInterleave { data: [0u8; 24] };

        let mut src_a = Self::g3B(p.data[0], p.data[1], p.data[2]);
        let mut src_b = Self::g3B(p.data[3], p.data[4], p.data[5]);
        let mut src_c = Self::g3B(p.data[6], p.data[7], p.data[8]);
        let mut src_d = Self::g3B(p.data[9], p.data[10], p.data[11]);
        let mut src_e = Self::g3B(p.data[12], p.data[13], p.data[14]);
        let mut src_f = Self::g3B(p.data[15], p.data[16], p.data[17]);
        let mut src_g = Self::g3B(p.data[18], p.data[19], p.data[20]);
        let mut src_h = Self::g3B(p.data[21], p.data[22], p.data[23]);

        for i in 0..24 {
            ret.data[i] = Self::g1b(src_a, 7)
                + Self::g1b(src_b, 6)
                + Self::g1b(src_c, 5)
                + Self::g1b(src_d, 4)
                + Self::g1b(src_e, 3)
                + Self::g1b(src_f, 2)
                + Self::g1b(src_g, 1)
                + Self::g1b(src_h, 0);

            src_a >>= 1;
            src_b >>= 1;
            src_c >>= 1;
            src_d >>= 1;
            src_e >>= 1;
            src_f >>= 1;
            src_g >>= 1;
            src_h >>= 1;
        }

        return ret;
    }

    fn deinterlace_single(src: u8) -> u8 {
        // b01...... prefix
        if (src >> 6) == 0b01 {
            return match src & 0b111111 {
                0b011001 => 0b000000,
                0b110001 => 0b000001,
                0b110010 => 0b000010,
                0b100101 => 0b000100,
                0b101001 => 0b001000,
                0b010011 => 0b010000,
                0b100011 => 0b100000,
                0b110100 => 0b110000,
                0b100110 => 0b111111,
                0b001110 => 0b111110,
                0b001101 => 0b111101,
                0b011010 => 0b111011,
                0b010110 => 0b110111,
                0b101100 => 0b101111,
                0b011100 => 0b011111,
                0b001011 => 0b001111,
                b => b,
            };
        } else {
            return src & 0b111111;
        }
    }

    fn _nb(w: u8, b: usize) -> u32 {
        ((w >> b) & 0x1) as u32
    }

    pub fn to(&self, p: &mut PacketWithGolay) {
        let mut d0: u32 = 0;
        let mut d1: u32 = 0;
        let mut d2: u32 = 0;
        let mut d3: u32 = 0;
        let mut d4: u32 = 0;
        let mut d5: u32 = 0;
        let mut d6: u32 = 0;
        let mut d7: u32 = 0;

        for src in 0..=23 {
            d0 <<= 1;
            d1 <<= 1;
            d2 <<= 1;
            d3 <<= 1;
            d4 <<= 1;
            d5 <<= 1;
            d6 <<= 1;
            d7 <<= 1;

            d0 |= Self::_nb(self.data[23 - src], 7);
            d1 |= Self::_nb(self.data[23 - src], 6);
            d2 |= Self::_nb(self.data[23 - src], 5);
            d3 |= Self::_nb(self.data[23 - src], 4);
            d4 |= Self::_nb(self.data[23 - src], 3);
            d5 |= Self::_nb(self.data[23 - src], 2);
            d6 |= Self::_nb(self.data[23 - src], 1);
            d7 |= Self::_nb(self.data[23 - src], 0);
        }

        for (dst, val) in [
            (0, d0),
            (3, d1),
            (6, d2),
            (9, d3),
            (12, d4),
            (15, d5),
            (18, d6),
            (21, d7),
        ] {
            p.data[dst] = (val >> 16) as u8;
            p.data[dst + 1] = (val >> 8) as u8;
            p.data[dst + 2] = val as u8;
        }
    }
}

// Use 6b/8b balanced code to remove DC offset (make the avg count of ones
// and zeros equal).
// https://en.wikipedia.org/wiki/6b/8b_encoding
const CODE_6TO8: [u8; 64] = [
    0b01011001_u8,
    0b01110001_u8,
    0b01110010_u8,
    0b11000011_u8,
    0b01100101_u8,
    0b11000101_u8,
    0b11000110_u8,
    0b10000111_u8,
    0b01101001_u8,
    0b11001001_u8,
    0b11001010_u8,
    0b10001011_u8,
    0b11001100_u8,
    0b10001101_u8,
    0b10001110_u8,
    0b01001011_u8,
    0b01010011_u8,
    0b11010001_u8,
    0b11010010_u8,
    0b10010011_u8,
    0b11010100_u8,
    0b10010101_u8,
    0b10010110_u8,
    0b00010111_u8,
    0b11011000_u8,
    0b10011001_u8,
    0b10011010_u8,
    0b00011011_u8,
    0b10011100_u8,
    0b00011101_u8,
    0b00011110_u8,
    0b01011100_u8,
    0b01100011_u8,
    0b11100001_u8,
    0b11100010_u8,
    0b10100011_u8,
    0b11100100_u8,
    0b10100101_u8,
    0b10100110_u8,
    0b00100111_u8,
    0b11101000_u8,
    0b10101001_u8,
    0b10101010_u8,
    0b00101011_u8,
    0b10101100_u8,
    0b00101101_u8,
    0b00101110_u8,
    0b01101100_u8,
    0b01110100_u8,
    0b10110001_u8,
    0b10110010_u8,
    0b00110011_u8,
    0b10110100_u8,
    0b00110101_u8,
    0b00110110_u8,
    0b01010110_u8,
    0b10111000_u8,
    0b00111001_u8,
    0b00111010_u8,
    0b01011010_u8,
    0b00111100_u8,
    0b01001101_u8,
    0b01001110_u8,
    0b01100110_u8,
];

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PacketWithoutDC {
    data: [u8; 32],
}

impl PacketWithoutDC {
    pub fn new(d: &[u8]) -> Self {
        let mut s = Self { data: [0; 32] };
        for b in d.iter().enumerate() {
            if b.0 >= s.data.len() {
                break;
            }
            s.data[b.0] = *b.1;
        }
        s
    }

    pub fn from(p: &PacketWithInterleave) -> PacketWithoutDC {
        let mut ret = PacketWithoutDC { data: [0u8; 32] };

        let mut buff: u16 = 0;
        let mut buff_cnt: u8 = 0;
        let mut src_next = 0;

        for i in 0..32 {
            // In LASO each 6 bit chunk is consumed from the first (lowest index) unconsumed byte's LSb side first,
            if buff_cnt < 6 {
                let src = p.data[src_next] as u16;
                src_next += 1;
                buff |= src << buff_cnt;
                buff_cnt += 8;
            }

            let idx = buff & 0x3f;
            buff >>= 6;
            buff_cnt -= 6;
            ret.data[i] = CODE_6TO8[idx as usize];
        }

        return ret;
    }

    pub fn to(&self, p: &mut PacketWithInterleave) {
        let mut buff: u16 = 0;
        let mut buff_cnt: u8 = 0;
        let mut dst_next = 0;

        for i in 0..self.data.len() {
            // In LASO each 6 bit chunk is consumed from the first (lowest index) unconsumed byte's LSb side first,
            let src = self.data[i];
            let dst = PacketWithInterleave::deinterlace_single(src) as u16;
            buff |= dst << buff_cnt;
            buff_cnt += 6;

            if buff_cnt >= 8 {
                let b = buff & 0xff;
                buff >>= 8;
                buff_cnt -= 8;
                p.data[dst_next] = b as u8;
                dst_next += 1;
            }
        }
    }

    pub fn data(&self) -> [u8; 32] {
        self.data
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use assert_hex::assert_eq_hex;
    use bitvec::prelude::*;

    #[test]
    fn test_interleave() {
        let pre = PacketWithGolay {
            data: [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96,
                0x87, 0x78, 0x69, 0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0xf, 0xcc,
            ],
        };
        let post = PacketWithInterleave::from(&pre);

        let expected: [u8; 24] = [
            0x2a, 0x8c, 0xdb, 0x47, 0xd4, 0x72, 0xa5, 0x79, 0x15, 0x59, 0x8b, 0x47, 0xea, 0xa6,
            0x34, 0x78, 0xa, 0xb3, 0x29, 0x67, 0xf5, 0x4c, 0x76, 0x38,
        ];
        assert_eq_hex!(
            post.data,
            expected,
            "Interleave does not work the same as the C code."
        );

        let mut pre_2 = PacketWithGolay { data: [0; 24] };
        post.to(&mut pre_2);
        assert_eq_hex!(pre_2, pre, "Interleave is not reversible");
    }

    #[test]
    fn test_6to8() {
        let pre = PacketWithInterleave {
            data: [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96,
                0x87, 0x78, 0x69, 0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0xf, 0xcc,
            ],
        };

        let expected: [u8; 32] = [
            0xd2, 0x53, 0xa3, 0x95, 0xb8, 0xa9, 0xc9, 0x6c, 0x1e, 0xc3, 0x5c, 0xb8, 0xd2, 0x4b,
            0xcc, 0x2d, 0xa5, 0x9a, 0x39, 0xe1, 0xb8, 0xa5, 0xa6, 0x96, 0x8b, 0xb1, 0x93, 0x8b,
            0x1e, 0x3c, 0x59, 0x33,
        ];

        let post = PacketWithoutDC::from(&pre);
        assert_eq_hex!(
            post.data,
            expected,
            "DC removal (6b to 8b) does not match LASO."
        );

        let mut pre2 = PacketWithInterleave { data: [0; 24] };
        post.to(&mut pre2);

        assert_eq_hex!(pre2.data, pre.data, "DC removal is not reversible");
    }

    #[test]
    fn test_6to8_reverse_internal() {
        let pre = PacketWithInterleave {
            data: [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96,
                0x87, 0x78, 0x69, 0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0xf, 0xcc,
            ],
        };

        let post = PacketWithoutDC::from(&pre);

        for (idx, (src, dest)) in post
            .data
            .view_bits::<Msb0>()
            .chunks(8)
            .zip(pre.data.view_bits::<Lsb0>().chunks(6))
            .enumerate()
        {
            let not_interleaved = dest.load_le::<u8>();
            let interleaved = src.load_be::<u8>();
            let reversed = PacketWithInterleave::deinterlace_single(interleaved);
            assert_eq_hex!(
                not_interleaved,
                reversed,
                "de-DC failed on {}. chunk ({:#08b} <-> {:#010b} <-> {:#08b})",
                idx,
                not_interleaved,
                interleaved,
                reversed
            );
        }
    }

    #[test]
    fn test_golay_reversibility() {
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            first: true,
            last: true,
        };
        let mut packet_2 = packet.clone();

        for v in [
            0x01_u8, 0x23_u8, 0x45_u8, 0x67_u8, 0x89_u8, 0xab_u8, 0xcd_u8, 0xef_u8, 0xf0_u8,
            0xe1_u8, 0xd2_u8,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        let p_w_golay = PacketWithGolay::from(&packet);

        let mut errors: usize = 0;
        p_w_golay.to(&mut packet_2, &mut errors);

        assert_eq_hex!(packet.data, packet_2.data, "Golay not reversible.");
        assert_eq_hex!(errors, 0, "Golay reversible with errors.");
    }

    #[test]
    fn test_packet() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            first: true,
            last: true,
        };
        for v in [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x74, packet.compute_cfg_byte(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);
        let p_w_interleave = PacketWithInterleave::from(&p_w_golay);
        let p_wo_dc = PacketWithoutDC::from(&p_w_interleave);

        let expected: [u8; 32] = [
            0xac, 0xe2, 0x3c, 0x96, 0x3a, 0x65, 0x35, 0x27, 0x8d, 0xb1, 0x33, 0xaa, 0xb1, 0xe8,
            0xa6, 0xc6, 0x72, 0xb2, 0x87, 0x2e, 0xd2, 0xa5, 0xa3, 0x99, 0xc9, 0x2d, 0xcc, 0xd8,
            0x17, 0x3c, 0xd4, 0xe8,
        ];

        assert_eq_hex!(
            p_wo_dc.data,
            expected,
            "Packet wire encoding does not match C LASO"
        );
    }

    #[test]
    fn test_dedc_single() {
        for (idx, b) in CODE_6TO8.iter().enumerate() {
            let deinterlaced = PacketWithInterleave::deinterlace_single(*b);
            assert_eq_hex!(
                deinterlaced,
                idx.as_u8(),
                "DC 6b to 8b table not reversible"
            );
        }
    }

    #[test]
    fn test_simple_packet() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            first: true,
            last: true,
        };
        for v in [
            0x81, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x24, packet.compute_cfg_byte(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);
        let p_w_interleave = PacketWithInterleave::from(&p_w_golay);
        let p_wo_dc = PacketWithoutDC::from(&p_w_interleave);

        let mut p_w_interleave2 = PacketWithInterleave { data: [0; 24] };
        p_wo_dc.to(&mut p_w_interleave2);

        assert_eq_hex!(p_w_interleave2.data, p_w_interleave.data);

        let mut p_w_golay2 = PacketWithGolay { data: [0; 24] };
        p_w_interleave2.to(&mut p_w_golay2);
        assert_eq_hex!(p_w_golay2.data, p_w_golay.data);

        let mut packet2 = PacketData {
            data: heapless::Vec::new(),
            first: true,
            last: true,
        };
        let mut err = 0;
        p_w_golay.to(&mut packet2, &mut err);

        assert_eq_hex!(packet2.data, packet.data);
    }

    #[test]
    fn test_golay_laso() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            first: true,
            last: true,
        };
        for v in [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x74, packet.compute_cfg_byte(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);

        let expected: [u8; 24] = [
            0x88, 0x51, 0x23, 0x5e, 0xa4, 0x56, 0x93, 0x67, 0x89, 0x21, 0xea, 0xbc, 0x4d, 0x6d,
            0xef, 0x62, 0x20, 0xe1, 0x6a, 0x9d, 0x2c, 0xed, 0x03, 0x74,
        ];

        assert_eq_hex!(p_w_golay.data, expected, "Golay does not match LASO");
    }

    #[test]
    fn test_golay_single() {
        assert_eq_hex!(
            PacketWithGolay::apply_golay(0x0_u16),
            0x0_u32,
            "Golay works differently from LASO"
        );
        assert_eq_hex!(
            PacketWithGolay::apply_golay(0x555_u16),
            0x4f4555_u32,
            "Golay works differently from LASO"
        );
        assert_eq_hex!(
            PacketWithGolay::apply_golay(0x123_u16),
            0x885123_u32,
            "Golay works differently from LASO"
        );
    }

    #[test]
    fn test_parity() {
        for i in 0..4096 {
            assert_eq!(PacketWithGolay::parity_24b(i), i.count_ones() & 0x1)
        }
    }

    #[test]
    #[cfg(feature = "fulltest")]
    fn test_golay_exhaustive() {
        for c in 0..4096 {
            for e1 in 0..23 {
                for e2 in 0..e1 {
                    for e3 in 0..e2 {
                        let mut cw = PacketWithGolay::apply_golay(c);
                        assert_eq!(
                            cw >> 23,
                            (cw & 0x7fffff).count_ones() & 0x1,
                            "Wrong parity in generated code"
                        );

                        let mask: u32 = (1 << e1) | (1 << e2) | (1 << e3);
                        cw.view_bits_mut::<Lsb0>()
                            .bitxor_assign(mask.view_bits::<Lsb0>());
                        let (c2, err, recovered) = PacketWithGolay::undo_golay(cw);

                        assert_eq!(c2, c, "Error correction failed.");
                        assert_eq!(
                            err,
                            mask.count_ones().as_usize(),
                            "Number of corrected errors does not match the error mask."
                        )
                    }
                }
            }
        }
    }
}

pub fn decode_extended_number<const N: usize>(data: &Vec<u8, N>, start: usize) -> (u16, usize) {
    // LSB first, MSb marks extended value
    let mut val = 0_u16;
    let mut shift = 0_u8;
    let mut idx = start;
    while (shift < 16 && idx < data.len()) {
        let b = data[idx] as u16;
        val += (b & 0x7F) << shift;
        shift += 7;
        idx += 1;

        if ((b & 0x80) == 0) {
            break;
        }
    }

    return (val, idx);
}
