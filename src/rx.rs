use crate::message::Message;
use crate::message::MessageVersion;
use crate::packet::GolayDecoderResult;
use crate::packet::PacketStatus;
use crate::util::decode_extended_number;

#[derive(Clone, Eq, PartialEq, Default)]
pub struct RxMessage<const N: usize> {
    pub msg: Message<N>,
    pub naked: bool,
    pub rssi: u8,
    pub lna: u8,
    pub errors: u8,

    last_status: PacketStatus,
    crc8: u8,
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
}

impl<const N: usize> RxMessage<N> {
    pub fn append(&mut self, dec: &GolayDecoderResult) -> Result<(), RxDecodeError> {
        let p = &dec.data;
        // Unexpected packet
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

        let mut skip = 0;

        match cur_status {
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
                } else {
                    self.msg.version = MessageVersion::V2;
                }
            }
            PacketStatus::CRC8P(crc) => {
                // TODO Test checksum
                // on fail return Err(CrcFailed)
            }
            PacketStatus::Unknown => return Err(RxDecodeError::UnknownPacket),
            PacketStatus::Raw(_) => return Err(RxDecodeError::RawNeedsDecoding),
            PacketStatus::Data(_) => {
                // Naked packet, ignore here and append data lower
            }
        };

        self.last_status = cur_status;

        for b in &p.data[skip..] {
            self.msg.data.push(*b).map_err(|_| RxDecodeError::Full)?;
        }

        if let PacketStatus::Data(b) = self.last_status {
            self.msg.data.push(b).map_err(|_| RxDecodeError::Full)?;
        }

        Ok(())
    }
}
