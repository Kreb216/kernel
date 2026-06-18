#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;
use alloc::alloc::Allocator;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::RefCell;
use core::ptr::NonNull;
use core::u8;

use ::core::ptr;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use pci_types::InterruptLine;
use rand_chacha::rand_core::block;
use smallvec::SmallVec;
use uart_16550::LoopbackError::SendError;
use virtio::blk::ConfigVolatileFieldAccess;
use virtio::{blk, le32, le64};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use super::virtio::virtqueue::VirtQueue;
use crate::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::Driver;
pub use crate::drivers::blk::error::VirtioBlkError;
use crate::drivers::virtio::ControlRegisters;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, Virtq,
};
use crate::mm::device_alloc::DeviceAlloc;

pub(crate) struct RequestQueue {
	vq: Option<VirtQueue>,
}

impl RequestQueue {
	pub fn new() -> Self {
		Self { vq: None }
	}

	pub fn add(&mut self, vq: VirtQueue) {
		self.vq = Some(vq);
	}
}

#[repr(C)]
pub struct ReqHdr {
	pub ty: le32,
	pub reserved: le32,
	pub sector: le64,
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct BlkDevCfg {
	pub raw: VolatileRef<'static, virtio::blk::Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::blk::F,
}

pub(crate) struct VirtioBlkDriver {
	pub(super) dev_cfg: BlkDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) irq: InterruptLine,

	pub(super) request_vq: RequestQueue,
}

impl Driver for VirtioBlkDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

impl VirtioBlkDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.acknowledge();

		#[cfg(not(feature = "pci"))]
		if status.contains(virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		#[cfg(feature = "pci")]
		if status.contains(virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioBlkError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///
	pub fn init_dev(&mut self) -> Result<(), VirtioBlkError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::blk::F::VERSION_1;
		let negotiated_features = self
			.com_cfg
			.control_registers()
			.negotiate_features(minimal_features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio block device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = negotiated_features;
		} else {
			error!("The device does not support our subset of features.");
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		//TODO: Device specific initialization
		// Create the queues and tell device about them
		self.request_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				0,
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));

		// Read block device size
		let block_size = self.dev_cfg.raw.as_ptr().capacity().read().to_ne() * 512;
		info!("Block device size: {block_size} bytes");

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		match self.test_device() {
			Ok(()) => info!("Test Succesful!"),
			Err(e) => {
				error!("Virtio-blk test failed: {e:?}");
				return Err(e);
			}
		}

		Ok(())
	}

	pub fn test_device(&mut self) -> Result<(), VirtioBlkError> {
		info!("Perform device test");

		let sector = le64::from_ne(3);

		let mut write_data = [0u8; 512];
		write_data[..4].copy_from_slice(b"test");

		info!("Writing to sector {sector:?}:");
		info!("Write bytes: {:02x?}", &write_data[..4]);

		self.write_sector(sector, &write_data)?;

		let mut read_buf = [0u8; 512];

		self.read_sector(sector, &mut read_buf)?;

		info!("Read from sector {sector:?}:");
		info!("Data bytes: {:02x?}", &read_buf[..4]);
		info!("Data text: {:?}", core::str::from_utf8(&read_buf[..4]));

		Ok(())
	}

	pub fn read_sector(&mut self, sector: le64, buf: &mut [u8; 512]) -> Result<(), VirtioBlkError> {
		let (hdr, data_vec, status) = self.alloc_req(virtio::blk::T::In, sector, buf)?;

		let mut send = SmallVec::new();
		send.push(BufferElem::Sized(hdr));

		let mut recv = SmallVec::new();
		recv.push(BufferElem::Vector(data_vec));
		recv.push(BufferElem::Sized(status));

		let mut request_result = self.send_req(send, recv)?;

		// Check status byte
		let recieved_data = request_result.used_recv_buff.pop_front_vec().unwrap();

		let recieved_status = request_result.used_recv_buff.pop_front_raw().unwrap();
		let status = recieved_status.0.downcast::<u8>().unwrap();

		if *status != virtio::blk::S::OK as u8 {
			info!("read status: {}", *status);
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		// Copy read data into the buffer
		buf.copy_from_slice(&recieved_data);

		Ok(())
	}

	pub fn write_sector(&mut self, sector: le64, buf: &[u8; 512]) -> Result<(), VirtioBlkError> {
		let (hdr, data_vec, status) = self.alloc_req(virtio::blk::T::Out, sector, buf)?;

		let mut send = SmallVec::new();
		send.push(BufferElem::Sized(hdr));
		send.push(BufferElem::Vector(data_vec));

		let mut recv = SmallVec::new();
		recv.push(BufferElem::Sized(status));

		let mut request_result = self.send_req(send, recv)?;

		// Check status byte
		let recieved_status = request_result.used_recv_buff.pop_front_raw().unwrap();
		let status = recieved_status.0.downcast::<u8>().unwrap();

		if *status != virtio::blk::S::OK as u8 {
			info!("write status: {}", *status);
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		Ok(())
	}

	fn alloc_req(
		&self,
		ty: virtio::blk::T,
		sector: le64,
		buf: &[u8],
	) -> Result<
		(
			Box<ReqHdr, DeviceAlloc>,
			Vec<u8, DeviceAlloc>,
			Box<u8, DeviceAlloc>,
		),
		VirtioBlkError,
	> {
		let hdr = ReqHdr {
			ty: (ty as u32).into(),
			reserved: le32::from_ne(0),
			sector,
		};

		let hdr_box = Box::new_in(hdr, DeviceAlloc);
		let data_vec = buf.to_vec_in(DeviceAlloc);
		let status = Box::new_in(0, DeviceAlloc);

		Ok((hdr_box, data_vec, status))
	}

	fn send_req(
		&mut self,
		send: SmallVec<[BufferElem; 2]>,
		recv: SmallVec<[BufferElem; 2]>,
	) -> Result<UsedBufferToken, VirtioBlkError> {
		let buffer_tkn = AvailBufferToken::new(send, recv).unwrap();
		let request_result = self
			.request_vq
			.vq
			.as_mut()
			.ok_or(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?
			.dispatch_blocking(buffer_tkn, BufferType::Direct)
			.map_err(|_| VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?;

		Ok(request_result)
	}
}

// Wrapper struct to use file system crate
pub struct SdmmcBlkAdapter<'a> {
	pub dev: RefCell<&'a mut VirtioBlkDriver>,
}

impl<'a> SdmmcBlkAdapter<'a> {
	pub fn new(dev: &'a mut VirtioBlkDriver) -> Self {
		Self {
			dev: RefCell::new(dev),
		}
	}
}

impl<'a> BlockDevice for SdmmcBlkAdapter<'a> {
	type Error = VirtioBlkError;

	fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
		let mut dev = self.dev.borrow_mut();

		for (i, block) in blocks.iter_mut().enumerate() {
			let sector = le64::from_ne(u64::from(start_block_idx.0) + i as u64);
			dev.read_sector(sector, &mut block.contents)?;
		}
		Ok(())
	}

	fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
		let mut dev = self.dev.borrow_mut();

		for (i, block) in blocks.iter().enumerate() {
			let sector = le64::from_ne(u64::from(start_block_idx.0) + i as u64);
			dev.write_sector(sector, &block.contents)?;
		}
		Ok(())
	}

	fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
		let dev = self.dev.borrow();
		let blocks = dev.dev_cfg.raw.as_ptr().capacity().read().to_ne();

		Ok(BlockCount(blocks as u32))
	}
}

/// Error module of virtio blk device driver.
pub mod error {
	use thiserror::Error;

	/// Virtio Blk device error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioBlkError {
		#[error("Virtio Blk device driver failed, for device {0:x}")]
		BlkDevError(u16),
	}
}
