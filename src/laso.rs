use crate::util::encode_varlength;

#[repr(u16)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum LasoPacketType {
    Unknown = 0x00,
    // TODO
    Temperature = 0x101,
    WaterLevel = 0x10A,
    GsmStatus = 0x102,
}

impl LasoPacketType {
    pub fn encode(self, consumer: impl FnMut(u8)) {
        let val = self as u16;
        encode_varlength(val as u32, consumer);
    }
}
