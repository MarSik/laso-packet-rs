use crate::util::encode_varlength;

#[repr(u32)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum LasoPacketType {
    Unknown = 0x00,
    // TODO
    Temperature = 0x1,
    WaterLevel = 0xA,
    GsmStatus = 0x2,
}

impl LasoPacketType {
    pub fn encode(self, consumer: impl FnMut(u8)) {
        encode_varlength(self.into(), consumer);
    }
}

impl From<LasoPacketType> for u32 {
    fn from(value: LasoPacketType) -> Self {
        value as u32
    }
}
