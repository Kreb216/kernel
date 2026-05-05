use virtio::{le16, le32, le64};

use crate::arch::pci::PciConfigRegion;
use crate::drivers::blk::{BlkDevCfg, RequestQueue, VirtioBlkDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

pub struct VirtioBlkGeometry {
	cylinders: le16,
	heads: u8,
	sectors: u8,
}

pub struct VirtioBlkTopology {
	// # of logical blocks per physical block (log2)
	physical_block_exp: u8,
	// offset of first aligned logical block
	alignment_offset: u8,
	// suggested minimum I/O size in blocks
	min_io_size: le16,
	// optimal (suggested maximum) I/O size in blocks
	opt_io_size: le32,
}

//TODO: Comment
pub(crate) struct BlkDevCfgRaw {
	pub capacity: le64,
	pub size_max: le32,
	pub seg_max: le32,
	pub geometry: VirtioBlkGeometry,
	pub blk_size: le32,
	pub topology: VirtioBlkTopology,
	pub writeback: u8,
	pub unused0: u8,
	pub num_queues: u8,
	pub max_discard_sectors: le32,
	pub max_discard_seg: le32,
	pub discard_sector_alignment: le32,
	pub max_write_zeroes_sectors: le32,
	pub max_write_zeroes_seg: le32,
	pub write_zeroes_may_unmap: u8,
	pub unused1: [u8; 3],
	pub max_secure_erase_sectors: le32,
	pub max_secure_erase_seg: le32,
	pub secure_erase_sector_alignment: le32,
}

impl VirtioBlkDriver {
	fn map_cfg(cap: &PciCap) -> Option<BlkDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<BlkDevCfgRaw>(cap)?;

		Some(BlkDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: , //TODO
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
