use virtio::{le16, le32, le64};
use volatile::VolatileRef;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::blk::{BlkDevCfg, RequestQueue, VirtioBlkDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

impl VirtioBlkDriver {
	fn map_cfg(cap: &PciCap) -> Option<BlkDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::blk::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(BlkDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::blk::F::empty(),
		})
	}

	/// TODO:Comment
	pub(crate) fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioBlkError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioBlkDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioBlkError::NoDevCfg(device_id));
		};

		Ok(VirtioBlkDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			irq: device.get_irq().unwrap(),
			request_vq: RequestQueue::new(), //TODO
		})
	}

	/// Initializes virtio block device
	///
	/// Returns a driver instance of VirtioBlkDriver.
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
					"Block device with device id {:x}, has been initialized by driver!",
					drv.dev_cfg.dev_id
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
