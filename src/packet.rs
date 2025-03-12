use heapless::Vec;
use ignore_result::Ignore;

use crate::dc::{balance, strip};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Default)]
pub struct PacketStatusLegacy {
    pub first: bool,
    pub last: bool,
    pub checksum4: u8,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Default)]

pub struct PacketStatusV2 {
    pub short: bool, // Just one packet

    // Header contains only Node ID and following packets have no CRC,
    // but use the last byte for data. This can also be called
    // the raw mode, because all handling is done in the higher
    // level app.
    pub naked: bool,

    // The transmitter will switch to receive mode after this packet
    // is sent. This can be used for commands or acks.
    pub listens: bool,
}
impl PacketStatusV2 {
    pub fn naked() -> PacketStatusV2 {
        Self {
            naked: true,
            ..Default::default()
        }
    }

    pub fn listens(self, listens: bool) -> Self {
        Self { listens, ..self }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Default)]
#[repr(u8)]
pub enum PacketStatus {
    // The original LASO packet format
    Legacy(PacketStatusLegacy),
    // Packets that support naked and listen modes
    V2(PacketStatusV2),
    // Continuation of V2 with just CRC8P
    CRC8P(u8),
    // Unknown, use as a start state while decoding
    #[default]
    Unknown,
    // Received data before decoding
    Raw(u8),
    // Naked mode, the status byte is used for extra payload data
    Data(u8),
    // Internally prepared, not for radio transmission
    Internal,
}

impl PacketStatus {
    pub fn finished(&self) -> bool {
        match self {
            PacketStatus::Legacy(legacy) => legacy.last,
            PacketStatus::V2(v2) => v2.short,
            PacketStatus::CRC8P(_) => false,
            PacketStatus::Unknown => true,
            PacketStatus::Raw(_) => false,
            PacketStatus::Data(_) => false,
            PacketStatus::Internal => true,
        }
    }

    pub fn decode(&self, next: u8) -> Self {
        match self {
            PacketStatus::Legacy(_) => PacketStatus::Legacy(PacketStatusLegacy {
                first: next & 0x4 > 0,
                last: next & 0x1 == 0,
                checksum4: next >> 4,
            }),
            PacketStatus::V2(v2) => {
                if v2.naked {
                    Self::Data(next)
                } else {
                    Self::CRC8P(next)
                }
            }
            PacketStatus::Unknown => {
                // The first packet in the legacy status mode always sets the "first" flag, use it to distinguish
                // the two versions
                if next & 0b100 > 0 {
                    // Legacy
                    PacketStatus::Legacy(PacketStatusLegacy {
                        first: next & 0x4 > 0,
                        last: next & 0x1 == 0,
                        checksum4: next >> 4,
                    })
                } else {
                    // V2
                    PacketStatus::V2(PacketStatusV2 {
                        short: next & 0x1 == 0,
                        listens: next & 0x8 > 0,
                        naked: next & 0x2 > 0,
                    })
                }
            }
            PacketStatus::CRC8P(_) => Self::CRC8P(next),
            PacketStatus::Raw(_) => Self::Raw(next),
            PacketStatus::Data(_) => Self::Data(next),
            PacketStatus::Internal => Self::Internal,
        }
    }

    pub fn encode(&self) -> u8 {
        match self {
            PacketStatus::Legacy(legacy) => {
                let mut flags: u8 = 0;
                if legacy.first {
                    flags += 0x4;
                }
                if !legacy.last {
                    flags += 0x1;
                }
                flags | (legacy.checksum4 << 4)
            }
            PacketStatus::V2(status_v2) => {
                let mut flags: u8 = 0;
                if status_v2.listens {
                    flags += 0x8;
                }
                if status_v2.naked {
                    flags += 0x2;
                }
                if !status_v2.short {
                    flags += 0x1;
                }
                flags
            }
            PacketStatus::CRC8P(crc) => *crc,
            PacketStatus::Unknown | PacketStatus::Internal => 0x00,
            PacketStatus::Raw(raw) => *raw,
            PacketStatus::Data(raw) => *raw,
        }
    }

    pub(crate) fn legacy(first: bool, last: bool) -> PacketStatus {
        PacketStatus::Legacy(PacketStatusLegacy {
            first,
            last,
            checksum4: 0,
        })
    }
}

#[derive(Default, Clone, Eq, PartialEq, Debug)]
pub struct PacketData {
    pub data: Vec<u8, 11>,
    pub status: PacketStatus,
}

impl PacketData {
    pub fn new() -> PacketData {
        PacketData {
            data: Vec::new(),
            status: PacketStatus::Unknown,
        }
    }

    fn checksum(acc: u8, v: &u8) -> u8 {
        acc.overflowing_add(*v).0
    }

    // Compare recomputed and current status and check packet for logical validity
    //
    // This is at the moment only effective for:
    // - Packets in legacy mode where each packet has a status byte and packet checksum
    // - The first packet in new protocol mode, the additional packet has only full message crc
    pub fn check_valid(&self) -> bool {
        self.compute_status() == self.status
    }

    // This is only effective for:
    // - Packets in legacy mode where each packet has a status byte
    // - The first packet in new protocol mode, the additional packets have no status
    pub fn compute_status(&self) -> PacketStatus {
        match self.status {
            PacketStatus::Legacy(legacy) => {
                let mut checksum8: u8 = self.data.iter().fold(0x55u8, Self::checksum);

                let strip_chsum = PacketStatus::Legacy(PacketStatusLegacy {
                    checksum4: 0,
                    ..legacy
                });

                checksum8 = checksum8.overflowing_add(strip_chsum.encode()).0;

                let ucrc = checksum8 >> 4;
                let lcrc = checksum8 & 0xf;
                let checksum4 = ucrc.overflowing_add(lcrc).0;

                PacketStatus::Legacy(PacketStatusLegacy {
                    checksum4,
                    ..legacy
                })
            }
            _ => self.status,
        }
    }

    pub(crate) fn to_wire_data(&self) -> [u8; 12] {
        let mut out = [0u8; 12];
        for (idx, v) in self.data.iter().enumerate() {
            out[idx] = *v;
        }
        out[11] = self.compute_status().encode();
        out
    }

    // Encode for transmit
    pub fn encode_for_transmit(&self) -> PacketWithoutDC {
        let p = PacketWithGolay::from(self);
        let p = PacketWithInterleave::from(&p);
        PacketWithoutDC::from(&p)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct PacketWithGolay {
    data: [u8; 24],
}

#[derive(Clone, Debug, Default)]
pub struct GolayDecoderResult {
    pub data: PacketData,
    pub parity_errors: usize,
    pub errors: usize,
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
        let code = (s as u32) | (c as u32);

        (Self::parity_24b(code) << 23) | code /* assemble codeword */
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
        (parity as u32) & 0x1_u32
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
}

impl From<&PacketWithGolay> for GolayDecoderResult {
    // Convert Golay encoded data into the final readable PacketData
    // Make sure the p.status is set to whatever the previous packet reported
    // to make sure the status type autodetection works correctly
    fn from(golay: &PacketWithGolay) -> Self {
        let mut ret = GolayDecoderResult::default();

        let mut buff = [0_u8; 12];

        let mut i_src = 0;
        let mut i_dst = 0;

        while i_src < golay.data.len() {
            let src1 = ((golay.data[i_src] as u32) << 16)
                + ((golay.data[i_src + 1] as u32) << 8)
                + (golay.data[i_src + 2] as u32);
            let src2 = ((golay.data[i_src + 3] as u32) << 16)
                + ((golay.data[i_src + 4] as u32) << 8)
                + (golay.data[i_src + 5] as u32);

            let (dst1, err1, parity1) = PacketWithGolay::undo_golay(src1);
            let (dst2, err2, parity2) = PacketWithGolay::undo_golay(src2);

            if !parity1 {
                ret.parity_errors += 1;
            }
            if !parity2 {
                ret.parity_errors += 1;
            }

            buff[i_dst] = (dst1 >> 4) as u8; // [12:4]
            buff[i_dst + 1] = (((dst1 & 0xf) << 4) as u8) + (((dst2 & 0xf00) >> 8) as u8); // [4:0] [12:8]
            buff[i_dst + 2] = dst2 as u8; // [8:0]

            ret.errors += err1 + err2;

            i_src += 6;
            i_dst += 3;
        }

        ret.data.data.clear();
        for i in 0..11 {
            // The destination is sized properly to take 11B
            ret.data.data.push(buff[i]).ignore();
        }
        ret.data.status = PacketStatus::Raw(buff[11]);

        ret
    }
}

impl From<&PacketData> for PacketWithGolay {
    fn from(p: &PacketData) -> Self {
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

        ret
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
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

    fn _nb(w: u8, b: usize) -> u32 {
        ((w >> b) & 0x1) as u32
    }
}

impl From<&PacketWithInterleave> for PacketWithGolay {
    fn from(p: &PacketWithInterleave) -> Self {
        let mut ret = PacketWithGolay::default();
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

            d0 |= PacketWithInterleave::_nb(p.data[23 - src], 7);
            d1 |= PacketWithInterleave::_nb(p.data[23 - src], 6);
            d2 |= PacketWithInterleave::_nb(p.data[23 - src], 5);
            d3 |= PacketWithInterleave::_nb(p.data[23 - src], 4);
            d4 |= PacketWithInterleave::_nb(p.data[23 - src], 3);
            d5 |= PacketWithInterleave::_nb(p.data[23 - src], 2);
            d6 |= PacketWithInterleave::_nb(p.data[23 - src], 1);
            d7 |= PacketWithInterleave::_nb(p.data[23 - src], 0);
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
            ret.data[dst] = (val >> 16) as u8;
            ret.data[dst + 1] = (val >> 8) as u8;
            ret.data[dst + 2] = val as u8;
        }

        ret
    }
}

impl From<&PacketWithGolay> for PacketWithInterleave {
    // Transform 8 24b src chunks into 24 8b dst chunks
    // [A23 .. A0][B23 .. B0] ... [H23 .. H0] -> [A0 B0 .. H0][A1 B1 .. H1] ... [A23 B23 .. H23]
    fn from(p: &PacketWithGolay) -> Self {
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
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
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

    pub fn data(&self) -> [u8; 32] {
        self.data
    }

    fn balance_dc(src: u8) -> u8 {
        balance(src)
    }

    fn strip_dc_balance_single(src: u8) -> u8 {
        strip(src)
    }
}

impl From<&PacketWithoutDC> for PacketWithInterleave {
    fn from(p: &PacketWithoutDC) -> Self {
        let mut ret = PacketWithInterleave::default();
        let mut buff: u16 = 0;
        let mut buff_cnt: u8 = 0;
        let mut dst_next = 0;

        for i in 0..p.data.len() {
            // In LASO each 6 bit chunk is consumed from the first (lowest index) unconsumed byte's LSb side first,
            let src = p.data[i];
            let dst = PacketWithoutDC::strip_dc_balance_single(src) as u16;
            buff |= dst << buff_cnt;
            buff_cnt += 6;

            if buff_cnt >= 8 {
                let b = buff & 0xff;
                buff >>= 8;
                buff_cnt -= 8;
                ret.data[dst_next] = b as u8;
                dst_next += 1;
            }
        }
        ret
    }
}

impl From<&PacketWithInterleave> for PacketWithoutDC {
    fn from(p: &PacketWithInterleave) -> PacketWithoutDC {
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
            ret.data[i] = PacketWithoutDC::balance_dc(idx as u8);
        }

        ret
    }
}

#[cfg(test)]
mod test {
    use core::error;

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

        let pre_2: PacketWithGolay = (&post).into();
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
            0x66, 0x64, 0xa3, 0x69, 0xd4, 0xb1, 0x31, 0x9b, 0x5a, 0x23, 0x5b, 0xd4, 0x66, 0x3b,
            0x38, 0x99, 0xa9, 0x72, 0xd1, 0xa5, 0xd4, 0xa9, 0xaa, 0x6a, 0x33, 0xe5, 0x63, 0x33,
            0x5a, 0xd8, 0x24, 0xe3,
        ];

        let post = PacketWithoutDC::from(&pre);
        assert_eq_hex!(
            post.data,
            expected,
            "DC removal (6b to 8b) does not match LASO."
        );

        let pre2: PacketWithInterleave = (&post).into();

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
            let with_dc = dest.load_le::<u8>();
            let without_dc = src.load_be::<u8>();
            let reversed = PacketWithoutDC::strip_dc_balance_single(without_dc);
            assert_eq_hex!(
                with_dc,
                reversed,
                "de-DC failed on {}. chunk ({:#08b} <-> {:#010b} <-> {:#08b})",
                idx,
                with_dc,
                without_dc,
                reversed
            );
        }
    }

    #[test]
    fn test_golay_reversibility() {
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            ..Default::default()
        };

        for v in [
            0x01_u8, 0x23_u8, 0x45_u8, 0x67_u8, 0x89_u8, 0xab_u8, 0xcd_u8, 0xef_u8, 0xf0_u8,
            0xe1_u8, 0xd2_u8,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        let p_w_golay = PacketWithGolay::from(&packet);

        let packet_2: GolayDecoderResult = (&p_w_golay).into();

        assert_eq_hex!(packet.data, packet_2.data.data, "Golay not reversible.");
        assert_eq_hex!(packet_2.errors, 0, "Golay reversible with errors.");
        assert_eq_hex!(
            packet_2.parity_errors,
            0,
            "Golay reversible with parity errors."
        );
    }

    #[test]
    fn test_packet() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            status: PacketStatus::Legacy(PacketStatusLegacy {
                first: true,
                last: true,
                checksum4: 0,
            }),
        };
        for v in [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x74, packet.compute_status().encode(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);
        let p_w_interleave = PacketWithInterleave::from(&p_w_golay);
        let p_wo_dc = PacketWithoutDC::from(&p_w_interleave);

        let expected: [u8; 32] = [
            0x98, 0xa6, 0xd8, 0x6a, 0xd2, 0x2c, 0xc9, 0xab, 0x39, 0xe5, 0xe3, 0xb2, 0xe5, 0xb4,
            0xaa, 0x2a, 0x26, 0xe6, 0x2b, 0x9a, 0x66, 0xa9, 0xa3, 0x71, 0x31, 0x99, 0x38, 0x74,
            0x6b, 0xd8, 0x6c, 0xb4,
        ];

        assert_eq_hex!(
            p_wo_dc.data,
            expected,
            "Packet wire encoding does not match LASO v2"
        );
    }

    #[test]
    fn test_simple_packet() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            status: PacketStatus::Legacy(PacketStatusLegacy {
                first: true,
                last: true,
                checksum4: 0,
            }),
        };
        for v in [
            0x81, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x24, packet.compute_status().encode(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);
        let p_w_interleave = PacketWithInterleave::from(&p_w_golay);
        let p_wo_dc = PacketWithoutDC::from(&p_w_interleave);

        let p_w_interleave2: PacketWithInterleave = (&p_wo_dc).into();

        assert_eq_hex!(p_w_interleave2.data, p_w_interleave.data);

        let p_w_golay2: PacketWithGolay = (&p_w_interleave2).into();
        assert_eq_hex!(p_w_golay2.data, p_w_golay.data);

        let packet2: GolayDecoderResult = (&p_w_golay).into();

        assert_eq_hex!(packet2.data.data, packet.data);
    }

    #[test]
    fn test_golay_laso() {
        // Prepare input packet data
        let mut packet = PacketData {
            data: heapless::Vec::new(),
            status: PacketStatus::Legacy(PacketStatusLegacy {
                first: true,
                last: true,
                checksum4: 0,
            }),
        };
        for v in [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0xe1, 0xd2, 0xc3,
        ] {
            packet.data.push(v).expect("Not enough space in vector");
        }

        assert_eq_hex!(0x74, packet.compute_status().encode(), "Bad flags byte.");

        let p_w_golay = PacketWithGolay::from(&packet);

        let expected: [u8; 24] = [
            0x88, 0x51, 0x23, 0x5e, 0xa4, 0x56, 0x93, 0x67, 0x89, 0x21, 0xea, 0xbc, 0x4d, 0x6d,
            0xef, 0x62, 0x20, 0xe1, 0x6a, 0x9d, 0x2c, 0xed, 0x03, 0x74,
        ];

        assert_eq_hex!(p_w_golay.data, expected, "Golay does not match LASO v2");
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

    #[test]
    fn test_v2_status_reversability() {
        // As first packet
        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: true,
            naked: false,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: false,
            naked: false,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: true,
            listens: true,
            naked: false,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: true,
            listens: false,
            naked: false,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: true,
            naked: true,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: false,
            naked: true,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: true,
            listens: true,
            naked: true,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: true,
            listens: false,
            naked: true,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        // As second packet
        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: true,
            naked: false,
        });
        assert_eq!(PacketStatus::CRC8P(0x55), status.decode(0x55));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: false,
            naked: false,
        });
        assert_eq!(PacketStatus::CRC8P(0x55), status.decode(0x55));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: true,
            naked: true,
        });
        assert_eq!(PacketStatus::Data(0x55), status.decode(0x55));

        let status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: false,
            naked: true,
        });
        assert_eq!(PacketStatus::Data(0x55), status.decode(0x55));
    }

    #[test]
    fn test_legacy_status_reversability() {
        // As first packet
        let status = PacketStatus::Legacy(PacketStatusLegacy {
            first: true,
            last: true,
            checksum4: 0x5,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        let status = PacketStatus::Legacy(PacketStatusLegacy {
            first: true,
            last: false,
            checksum4: 0x5,
        });
        assert_eq!(status, PacketStatus::Unknown.decode(status.encode()));

        // As second packet
        let status = PacketStatus::Legacy(PacketStatusLegacy {
            first: false,
            last: true,
            checksum4: 0x5,
        });
        assert_eq!(status, status.decode(status.encode()));

        let status = PacketStatus::Legacy(PacketStatusLegacy {
            first: false,
            last: false,
            checksum4: 0x5,
        });
        assert_eq!(status, status.decode(status.encode()));
    }

    #[test]
    fn test_crc_status_reversability() {
        // As second packet (crc is never present as first packet)
        let first_status = PacketStatus::V2(PacketStatusV2 {
            short: false,
            listens: false,
            naked: false,
        });
        let status = PacketStatus::CRC8P(0x32);
        assert_eq!(status, first_status.decode(status.encode()));

        // As third packet (crc is never present as first packet)
        let status = PacketStatus::CRC8P(0x32);
        assert_eq!(status, status.decode(status.encode()));
    }

    #[test]
    fn test_full_6to8_reversability() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = PacketWithoutDC::balance_dc(b);
            let decoded = PacketWithoutDC::strip_dc_balance_single(encoded);
            assert_eq!(b, decoded, "6 to 8 reversability broken for {}", b);
        }
    }

    #[test]
    fn test_one_bit_dc_error_impact() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = PacketWithoutDC::balance_dc(b);
            for i in 0..8 {
                let xor = 1_u8 << i;
                let decoded = PacketWithoutDC::strip_dc_balance_single(encoded ^ xor);
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
    fn test_two_bit_dc_error_impact() {
        // Test each 6b symbol
        for b in 0_u8..=0x3f {
            let encoded = PacketWithoutDC::balance_dc(b);
            for i in 0..8 {
                for j in 0..8 {
                    if i == j {
                        continue;
                    }

                    let xor = 1_u8 << i | 1_u8 << j;
                    let decoded = PacketWithoutDC::strip_dc_balance_single(encoded ^ xor);
                    let error_bits = b ^ decoded;
                    assert!(error_bits.count_ones() <= 2, "6 to 8 reverse broken with {} bit errors in 0x{:x} and bitflip mask 0x{:x}", error_bits.count_ones(), b, xor);
                }
            }
        }
    }
}
