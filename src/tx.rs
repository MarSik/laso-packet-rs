use ignore_result::Ignore as _;

use crate::{
    message::{BitAdder as _, Message},
    packet::{PacketData, PacketStatus, PacketStatusV2},
    util::{encode_id, encode_varlength},
};

#[derive(Clone, Eq, PartialEq)]
pub struct MessageSender<const N: usize> {
    message: Message<{ N }>,
    // Status template for the next generated packet
    next_status: PacketStatus,
    sent: usize,
}

impl<const N: usize> MessageSender<N> {
    pub fn new(message: Message<N>) -> Self {
        let version = message.version;
        let listens = message.will_listen;
        Self {
            message,
            next_status: match version {
                crate::message::MessageVersion::LegacyLaso => PacketStatus::legacy(true, true),
                crate::message::MessageVersion::V2 => {
                    PacketStatus::V2(PacketStatusV2::default().listens(listens))
                }
                crate::message::MessageVersion::Naked => {
                    PacketStatus::V2(PacketStatusV2::naked().listens(listens))
                }
            },
            sent: 0,
        }
    }

    pub fn data_to_send(&self) -> bool {
        self.sent < self.message.data.len()
    }

    pub fn packet(&mut self) -> PacketData {
        let mut p = PacketData::new();

        p.status = self.next_status;

        match p.status {
            PacketStatus::Legacy(legacy) => {
                if legacy.first {
                    // Queue source address and packet type
                    // First packet always has enough space for this
                    p.data
                        .add(encode_id(self.message.packet_type.unwrap_or(0x00)));
                    encode_varlength(self.message.source_address as u32, |b| {
                        p.data.push(b).ignore();
                    });
                }

                self.next_status = PacketStatus::legacy(false, true);
            }
            PacketStatus::V2(v2) => {
                if !v2.naked {
                    // Queue source address and packet type
                    // First packet always has enough space for this
                    p.data
                        .add(encode_id(self.message.packet_type.unwrap_or(0x00)));
                }

                encode_varlength(self.message.source_address as u32, |b| {
                    p.data.push(b).ignore();
                });

                if v2.naked {
                    self.next_status = PacketStatus::Data(0x00);
                } else {
                    self.next_status = PacketStatus::CRC8P(0x00);
                }
            }
            // The following are end states, no change for follow-up packets
            PacketStatus::CRC8P(_) => (),
            PacketStatus::Unknown => (),
            PacketStatus::Raw(_) => (),
            PacketStatus::Data(_) => (),
        };

        while !p.data.is_full() && self.sent < self.message.data.len() {
            p.data
                .push(*self.message.data.get(self.sent).unwrap())
                .unwrap();
            self.sent += 1;
        }

        while !p.data.is_full() {
            p.data.push(0u8).unwrap();
        }

        // Fill in continuation markers
        if let PacketStatus::Legacy(legacy) = &mut p.status {
            legacy.last = !self.data_to_send();
        }
        if let PacketStatus::V2(v2) = &mut p.status {
            v2.short = !self.data_to_send();
        }

        // Add one extra data byte when in naked mode
        if let PacketStatus::Data(data) = &mut p.status {
            *data = *self.message.data.get(self.sent).unwrap_or(&0x00);
            self.sent += 1;
        }

        // Re-compute packet level status
        p.status = p.compute_status();

        p
    }
}
