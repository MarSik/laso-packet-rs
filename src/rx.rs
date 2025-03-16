use crc::Algorithm;
use crc::Digest;
use crc::NoTable;

use crate::message::Message;
use crate::message::MessageVersion;
use crate::packet::GolayDecoderResult;
use crate::packet::PacketStatus;
use crate::util::decode_extended_number;

const CRC8K_3: Algorithm<u8> = Algorithm {
    width: 8,
    poly: 0xd5,
    init: 0x00,
    refin: false,
    refout: false,
    xorout: 0x00,
    check: 0x00,
    residue: 0x00,
};
pub const LASO_CRC: crc::Crc<u8, NoTable> = crc::Crc::<u8, NoTable>::new(&CRC8K_3);

#[derive(Clone)]
pub struct RxMessage<'a, const N: usize> {
    pub msg: Message<N>,
    pub naked: bool,
    pub rssi: u8,
    pub lna: u8,
    pub errors: u8,

    last_status: PacketStatus,
    crc8: Digest<'a, u8, NoTable>,
}

impl<'a, const N: usize> Default for RxMessage<'a, N> {
    fn default() -> Self {
        Self {
            crc8: LASO_CRC.digest(),
            msg: Default::default(),
            naked: Default::default(),
            rssi: Default::default(),
            lna: Default::default(),
            errors: Default::default(),
            last_status: Default::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RxDecodeError {
    OutOfOrder,
    Unexpected,
    Invalid,
    CrcFailed,
    Full,
    UnknownPacket,
    RawNeedsDecoding,
    InternalOnly,
}

impl<'a, const N: usize> RxMessage<'a, N> {
    pub fn append(&mut self, dec: &GolayDecoderResult) -> Result<(), RxDecodeError> {
        let p = &dec.data;
        // Unexpected packet
        #[cfg(feature = "legacy")]
        if let PacketStatus::Legacy(legacy) = self.last_status {
            if legacy.last {
                return Err(RxDecodeError::Unexpected);
            }
        }

        if let PacketStatus::V2(v2) = self.last_status {
            if v2.short {
                return Err(RxDecodeError::Unexpected);
            }
        }

        // Decode raw status
        let cur_status = if let PacketStatus::Raw(raw) = p.status {
            self.last_status.decode(raw)
        } else {
            p.status
        };

        // Check internal packet validity
        if !p.check_valid() {
            return Err(RxDecodeError::Invalid);
        }

        // How many bytes were already consumed
        // from the received data for headers and
        // protocol
        let mut skip = 0;

        // How many data bytes are present in the
        // received message, including `skip`
        let mut size: usize = p.data.len();

        match cur_status {
            #[cfg(feature = "legacy")]
            PacketStatus::Legacy(legacy) => {
                // First packet flag when data already recorded?
                if !self.msg.data.is_empty() && legacy.first {
                    return Err(RxDecodeError::OutOfOrder);
                }

                // Checksum was tested as part of Packet.check_valid()
                // above.

                if legacy.first {
                    let packet_type;
                    (packet_type, skip) = decode_extended_number(dec.data.data.as_slice(), skip);
                    self.msg.packet_type = Some(packet_type);
                    (self.msg.source_address, skip) =
                        decode_extended_number(dec.data.data.as_slice(), skip);
                }

                self.msg.version = MessageVersion::LegacyLaso;
            }
            PacketStatus::V2(v2) => {
                self.naked = v2.naked;

                let packet_type;
                if !self.naked {
                    (packet_type, skip) = decode_extended_number(dec.data.data.as_slice(), skip);
                    self.msg.packet_type = Some(packet_type);
                }
                (self.msg.source_address, skip) =
                    decode_extended_number(dec.data.data.as_slice(), skip);

                if self.naked {
                    self.msg.version = MessageVersion::Naked;
                } else if v2.short {
                    self.msg.version = MessageVersion::V2Short;
                    // Subtract 1 from size, the last data byte contains CRC
                    // for the short packet
                    size -= 1;
                } else {
                    self.msg.version = MessageVersion::V2;
                }

                // Feed data into CRC, including status byte
                self.crc8.update(&p.data[..size]);
                self.crc8.update(&[p.status.encode()]);

                if v2.short {
                    // This is fine, because 1 was subtracted
                    // from size above. It now points to the last
                    // byte that contains CRC
                    let crc = p.data[size];

                    // Test checksum without modifying the digest
                    // this allows using the same running digest
                    // for followup packets
                    if crc != self.crc8.clone().finalize() {
                        return Err(RxDecodeError::CrcFailed);
                    }
                }
            }
            PacketStatus::CRC8P(crc) => {
                // Feed data into CRC, excluding status byte!
                self.crc8.update(&p.data);

                // Test checksum without modifying the digest
                // this allows using the same running digest
                // for followup packets
                if crc != self.crc8.clone().finalize() {
                    return Err(RxDecodeError::CrcFailed);
                }
            }
            PacketStatus::Unknown => return Err(RxDecodeError::UnknownPacket),
            PacketStatus::Internal => return Err(RxDecodeError::InternalOnly),
            PacketStatus::Raw(_) => return Err(RxDecodeError::RawNeedsDecoding),
            PacketStatus::Data(_) => {
                // Naked packet, ignore here and append data lower
            }
        };

        self.last_status = cur_status;
        self.errors = self.errors.saturating_add(dec.errors as u8);
        self.errors = self.errors.saturating_add(dec.parity_errors as u8);

        for b in &p.data[skip..size] {
            self.msg.data.push(*b).map_err(|_| RxDecodeError::Full)?;
        }

        if let PacketStatus::Data(b) = self.last_status {
            self.msg.data.push(b).map_err(|_| RxDecodeError::Full)?;
        }

        Ok(())
    }
}

impl<'a, const N: usize> From<Message<N>> for RxMessage<'a, N> {
    fn from(msg: Message<N>) -> Self {
        Self {
            msg,
            last_status: PacketStatus::Internal,
            ..Default::default()
        }
    }
}
