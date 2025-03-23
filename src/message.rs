use core::ops::Shr;

use heapless::Vec;
use ignore_result::Ignore as _;
use ufmt::derive::uDebug;

use crate::{
    tx::MessageSender,
    util::{encode_varlength, IntoLeastSigByte},
};

#[derive(uDebug, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MessageVersion {
    #[cfg(feature = "legacy")]
    LegacyLaso,
    #[default]
    V2,
    V2Short,
    Naked,
}

// Message builder with flags
// This is also used for reception via the RxMessage struct
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct Message<const N: usize> {
    pub version: MessageVersion,
    pub data: Vec<u8, { N }>,
    pub source_address: u32,
    pub packet_type: Option<u32>,
    pub will_listen: bool,
}

impl<const N: usize> Message<N> {
    pub fn sender<'a>(self) -> MessageSender<'a, { N }> {
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
            self.push(b8).ignore();
        }
    }

    fn add_varlen(&mut self, v: u32) {
        encode_varlength(v, |b| {
            self.push(b).ignore();
        });
    }
}
