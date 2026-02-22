use crate::virtio_block::VirtioHal;
use virtio_drivers::{
    device::rng::VirtIORng,
    transport::mmio::MmioTransport,
};

/// VirtIO RNG device wrapper
pub struct VirtioRng(VirtIORng<VirtioHal, MmioTransport<'static>>);

impl VirtioRng {
    pub fn new(transport: MmioTransport<'static>) -> Result<Self, virtio_drivers::Error> {
        let rng = VirtIORng::new(transport)?;
        Ok(Self(rng))
    }

    pub fn read(&mut self, data: &mut [u8]) -> usize {
        let mut count = 0;
        while count < data.len() {
            match self.0.request_entropy(&mut data[count..]) {
                Ok(n) => count += n,
                Err(_) => break,
            }
        }
        count
    }
}

// Safety: Mutex in main.rs protecting access
unsafe impl Send for VirtioRng {}
unsafe impl Sync for VirtioRng {}
