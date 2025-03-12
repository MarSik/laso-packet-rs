use core::ops::Shr;

use heapless::Vec;

use crate::{
    tx::MessageSender,
    util::{encode_varlength, IntoLeastSigByte},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MessageVersion {
    #[default]
    LegacyLaso,
    V2,
    Naked,
}

// Message builder with flags
// This is also used for reception via the RxMessage struct
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct Message<const N: usize> {
    pub version: MessageVersion,
    pub data: Vec<u8, { N }>,
    pub source_address: u16,
    pub packet_type: Option<u16>,
    pub will_listen: bool,
}

impl<const N: usize> Message<N> {
    pub fn sender(self) -> MessageSender<{ N }> {
        MessageSender::new(self)
    }

    pub fn add<T: Shr<usize, Output = T> + Into<IntoLeastSigByte> + Copy>(&mut self, v: T) {
        self.data.add(v);
    }

    pub fn add_varlen(&mut self, v: u32) {
        self.data.add_varlen(v);
    }
}

pub trait BitAdder {
    fn add<T: Shr<usize, Output = T> + Into<IntoLeastSigByte> + Copy>(&mut self, v: T);
    fn add_varlen(&mut self, v: u32);
}

impl<const N: usize> BitAdder for Vec<u8, { N }> {
    fn add<T: Shr<usize, Output = T> + Into<IntoLeastSigByte> + Copy>(&mut self, v: T) {
        let mut bits = size_of::<T>() * 8;
        while bits >= 8 {
            bits -= 8;
            let bw = (v >> bits).into();
            let b8 = bw.into();
            self.push(b8);
        }
    }

    fn add_varlen(&mut self, v: u32) {
        encode_varlength(v, |b| {
            self.push(b);
        });
    }
}
