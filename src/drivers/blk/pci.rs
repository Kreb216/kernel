use crate::arch::pci::PciConfigRegion;
use crate::drivers::blk::VirtioBlkDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;

impl VirtioBlkDriver {
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioBlkDriver, VirtioError> {
		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioBlkDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(blk_err) => {
					error!("Initializing new virtio block device driver failed. Aborting!");
					return Err(VirtioError::VirtioBlkDriver(blk_err));
				}
			},
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => {
				info!(
					"Block device with cid {:x}, has been initialized by driver!",
					drv.dev_cfg.raw.guest_cid
				);

				Ok(drv)
			}
			Err(err) => {
				drv.set_failed();
				Err(VirtioError::VirtioBlkDriver(err))
			}
		}
	}
}
