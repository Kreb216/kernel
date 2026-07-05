use embedded_sdmmc::{Block, BlockDevice, BlockIdx, Mode, VolumeIdx, VolumeManager};
use virtio::le64;

pub use crate::drivers::blk::error::VirtioBlkError;
use crate::drivers::blk::{DummyTimeSource, SdmmcBlkAdapter, VirtioBlkDriver};
use crate::processor;

impl VirtioBlkDriver {
	pub fn test_device(&mut self) -> Result<(), VirtioBlkError> {
		info!("Perform device test");

		let sector = le64::from_ne(3);

		let mut write_data = [0u8; 512];
		write_data[..4].copy_from_slice(b"test");

		info!("Writing to sector {sector:?}:");
		info!("Write bytes: {:02x?}", &write_data[..4]);

		self.write_sectors(sector, &write_data)?;

		let mut read_buf = [0u8; 512];

		self.read_sectors(sector, &mut read_buf)?;

		info!("Read from sector {sector:?}:");
		info!("Data bytes: {:02x?}", &read_buf[..4]);
		info!("Data text: {:?}", core::str::from_utf8(&read_buf[..4]));

		Ok(())
	}

	pub fn test_sdmmc_adapter(&mut self) -> Result<(), VirtioBlkError> {
		info!("Testing SdmmcBlkAdapter");

		let adapter = SdmmcBlkAdapter::new(self);

		let mut write_blocks = [Block::new(); 1];
		write_blocks[0].contents[..4].copy_from_slice(b"sdmc");

		adapter.write(&write_blocks, BlockIdx(4))?;

		let mut read_blocks = [Block::new(); 1];
		adapter.read(&mut read_blocks, BlockIdx(4))?;

		info!("Adapter read bytes: {:02x?}", &read_blocks[0].contents[..4]);
		info!(
			"Adapter read text: {:?}",
			core::str::from_utf8(&read_blocks[0].contents[..4])
		);

		if &read_blocks[0].contents[..4] != b"sdmc" {
			return Err(VirtioBlkError::BlkDevError(
				adapter.dev.borrow().dev_cfg.dev_id,
			));
		}

		Ok(())
	}

	pub fn test_fat(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;

		let file_name = "OHA.txt";
		let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

		const TEXT: &[u8] = b"hello from virtio blk\n";
		file.write(TEXT)?;
		file.flush()?;
		file.close()?;

		let file_ro = root_dir.open_file_in_dir(file_name, Mode::ReadOnly)?;
		while !file_ro.is_eof() {
			let mut buf = [0u8; TEXT.len()];
			let num_read = file_ro.read(&mut buf)?;

			info!("Contents of: {file_name}");
			for b in &buf[..num_read] {
				info!("{}", *b as char);
			}
		}
		Ok(())
	}

	//RAW
	pub fn test_correctness_raw(&mut self) -> Result<(), VirtioBlkError> {
		//Prep
		const TOTAL_SIZE: usize = 1 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks
		const SECTOR_SIZE: usize = 512;
		const START_SECTOR: u64 = 2048; // 1 MiB starting offset
		const SECTOR_PER_CHUNK: u64 = (CHUNK_SIZE / SECTOR_SIZE) as u64;

		info!("Starting raw correctness test:");

		//Write
		{
			let mut sector = START_SECTOR;
			let mut write_buf = [0u8; CHUNK_SIZE];

			let mut written_size = 0;
			let mut chunk_idx = 0;

			while written_size < TOTAL_SIZE {
				Self::fill_buffer_for_correctness(&mut write_buf, chunk_idx);

				self.write_sectors(le64::from_ne(sector), &write_buf)?;

				written_size += CHUNK_SIZE;
				sector += SECTOR_PER_CHUNK;
				chunk_idx += 1;
			}
		}

		//Read
		{
			let mut sector = START_SECTOR;
			let mut read_buf = [0u8; CHUNK_SIZE];
			let mut expected_buf = [0u8; CHUNK_SIZE];

			let mut read_size = 0;
			let mut chunk_idx = 0;

			//Test
			while read_size < TOTAL_SIZE {
				Self::fill_buffer_for_correctness(&mut expected_buf, chunk_idx);
				self.read_sectors(le64::from_ne(sector), &mut read_buf)?;

				if expected_buf != read_buf {
					error!("Mismatch in chunk{}", chunk_idx);
					return Err(VirtioBlkError::BlkDevError(self.get_dev_id()));
				}

				read_size += CHUNK_SIZE;
				sector += SECTOR_PER_CHUNK;
				chunk_idx += 1;
			}
		}
		//Output

		info!("Raw corectness test finished:");

		Ok(())
	}

	pub fn test_seq_write_raw(&mut self) -> Result<(), VirtioBlkError> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 64 * 1024; // KiB chunks
		const SECTOR_SIZE: usize = 512;
		const START_SECTOR: u64 = 2048; // 1 MiB starting offset
		const SECTOR_PER_CHUNK: u64 = (CHUNK_SIZE / SECTOR_SIZE) as u64;

		let mut sector = START_SECTOR;

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let mut buf = [0u8; CHUNK_SIZE];
		Self::fill_buffer(&mut buf);

		info!("Starting sequential raw write test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		let mut written_size = 0;
		//Test
		let start = processor::get_timer_ticks();

		while written_size < TOTAL_SIZE {
			self.write_sectors(le64::from_ne(sector), &buf)?;
			written_size += CHUNK_SIZE;
			sector += SECTOR_PER_CHUNK;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;

		info!("Sequential raw write test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);

		Ok(())
	}

	pub fn test_seq_read_raw(&mut self) -> Result<(), VirtioBlkError> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 64 * 1024; // KiB chunks
		const SECTOR_SIZE: usize = 512;
		const START_SECTOR: u64 = 2048; // 1 MiB starting offset
		const SECTOR_PER_CHUNK: u64 = (CHUNK_SIZE / SECTOR_SIZE) as u64;

		let mut sector = START_SECTOR;

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		info!("Starting sequential raw read test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		{
			let mut write_buf = [0u8; CHUNK_SIZE];
			Self::fill_buffer(&mut write_buf);

			let mut written_size = 0;

			while written_size < TOTAL_SIZE {
				self.write_sectors(le64::from_ne(sector), &write_buf)?;
				written_size += CHUNK_SIZE;
				sector += SECTOR_PER_CHUNK;
			}

			sector = START_SECTOR;
		}

		let mut read_buf = [0u8; CHUNK_SIZE];
		let mut read_size = 0;

		//Test
		let start = processor::get_timer_ticks();

		while read_size < TOTAL_SIZE {
			self.read_sectors(le64::from_ne(sector), &mut read_buf)?;
			read_size += CHUNK_SIZE;
			sector += SECTOR_PER_CHUNK;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;

		info!("Sequential raw read test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);

		Ok(())
	}

	pub fn test_rnd_write_raw(&mut self) -> Result<(), VirtioBlkError> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks
		const SECTOR_SIZE: usize = 512;
		const START_SECTOR: u64 = 2048; // 1 MiB starting offset
		const SECTOR_PER_CHUNK: u64 = (CHUNK_SIZE / SECTOR_SIZE) as u64;
		const OP_AMOUNT: usize = TOTAL_SIZE / CHUNK_SIZE;

		let mut sectors = [0u64; OP_AMOUNT];

		for (i, sector) in sectors.iter_mut().enumerate() {
			let chunk_idx = Self::access_pattern(OP_AMOUNT, i);
			*sector = START_SECTOR + chunk_idx * SECTOR_PER_CHUNK;
		}

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let mut buf = [0u8; CHUNK_SIZE];
		Self::fill_buffer(&mut buf);

		info!("Starting random raw write test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		//Test
		let start = processor::get_timer_ticks();

		for sector in sectors {
			self.write_sectors(le64::from_ne(sector), &buf)?;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;
		let iops = OP_AMOUNT as f64 / elapsed_seconds;

		info!("Random write test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);
		info!("IOPS: {:.3}", iops);

		Ok(())
	}

	pub fn test_rnd_read_raw(&mut self) -> Result<(), VirtioBlkError> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks
		const SECTOR_SIZE: usize = 512;
		const START_SECTOR: u64 = 2048; // 1 MiB starting offset
		const SECTOR_PER_CHUNK: u64 = (CHUNK_SIZE / SECTOR_SIZE) as u64;
		const OP_AMOUNT: usize = TOTAL_SIZE / CHUNK_SIZE;

		let mut sectors = [0u64; OP_AMOUNT];

		for (i, sector) in sectors.iter_mut().enumerate() {
			let chunk_idx = Self::access_pattern(OP_AMOUNT, i);
			*sector = START_SECTOR + chunk_idx * SECTOR_PER_CHUNK;
		}

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		{
			let mut write_buf = [0u8; CHUNK_SIZE];
			Self::fill_buffer(&mut write_buf);

			for sector in sectors {
				self.write_sectors(le64::from_ne(sector), &write_buf)?;
			}
		}

		let mut read_buf = [0u8; CHUNK_SIZE];

		info!("Starting random raw read test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		//Test
		let start = processor::get_timer_ticks();

		for sector in sectors {
			self.read_sectors(le64::from_ne(sector), &mut read_buf)?;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;
		let iops = OP_AMOUNT as f64 / elapsed_seconds;

		info!("Random read test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);
		info!("IOPS: {:.3}", iops);

		Ok(())
	}

	//SDMMC
	pub fn test_correctness_sdmmc(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		//Prep
		const TOTAL_SIZE: usize = 1 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks

		let dev_id = self.get_dev_id();

		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;
		let file_name = "CORRECT.BIN";

		//Write
		{
			let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

			let mut write_buf = [0u8; CHUNK_SIZE];
			let mut chunk_idx = 0;

			let mut written_size = 0;

			while written_size < TOTAL_SIZE {
				Self::fill_buffer_for_correctness(&mut write_buf, chunk_idx);

				file.write(&write_buf)?;

				written_size += CHUNK_SIZE;
				chunk_idx += 1;
			}

			file.flush()?;
			file.close()?;
		}

		//Read
		{
			let file = root_dir.open_file_in_dir(file_name, Mode::ReadOnly)?;

			let mut expected_buf = [0u8; CHUNK_SIZE];
			let mut read_buf = [0u8; CHUNK_SIZE];
			let mut chunk_idx = 0;

			let mut read_size = 0;

			while read_size < TOTAL_SIZE {
				Self::fill_buffer_for_correctness(&mut expected_buf, chunk_idx);

				read_size += file.read(&mut read_buf)?;

				if expected_buf != read_buf {
					error!("Mismatch in chunk{}", chunk_idx);
					return Err(embedded_sdmmc::Error::DeviceError(
						VirtioBlkError::BlkDevError(dev_id),
					));
				}

				chunk_idx += 1;
			}
		}

		//Output
		info!("Sdmmc correctness finished:");

		Ok(())
	}

	pub fn test_seq_write_sdmmc(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		//Prep
		const TOTAL_SIZE: usize = 16 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 512; // chunks

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;
		let file_name = "SEQWRITE.BIN";
		let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

		let mut buf = [0u8; CHUNK_SIZE];
		Self::fill_buffer(&mut buf);

		info!("Starting sequential write test:");
		info!("Path: /{}", file_name);
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		let start = processor::get_timer_ticks();

		let mut written_size = 0;
		//Test

		while written_size < TOTAL_SIZE {
			file.write(&buf)?;
			written_size += CHUNK_SIZE;
		}

		let end = processor::get_timer_ticks();
		//Output

		file.flush()?;
		file.close()?;

		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;

		info!("Sequential write test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);

		Ok(())
	}

	pub fn test_seq_read_sdmmc(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		//Prep
		const TOTAL_SIZE: usize = 16 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 512; // KiB chunks

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;
		let file_name = "SEQREAD.BIN";

		{
			let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

			let mut write_buf = [0u8; CHUNK_SIZE];
			Self::fill_buffer(&mut write_buf);

			let mut written_size = 0;

			while written_size < TOTAL_SIZE {
				file.write(&write_buf)?;
				written_size += CHUNK_SIZE;
			}

			file.flush()?;
			file.close()?;
		}

		let file = root_dir.open_file_in_dir(file_name, Mode::ReadOnly)?;

		let mut read_buf = [0u8; CHUNK_SIZE];

		info!("Starting sequential read test:");
		info!("Path: /{}", file_name);
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		let start = processor::get_timer_ticks();

		let mut read_size = 0;
		//Test

		while read_size < TOTAL_SIZE {
			read_size += file.read(&mut read_buf)?;
		}
		//Output
		let end = processor::get_timer_ticks();

		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;

		info!("Sequential read finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);

		Ok(())
	}

	pub fn test_rnd_write_sdmmc(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks
		const OP_AMOUNT: usize = TOTAL_SIZE / CHUNK_SIZE;

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let mut offsets = [0u32; OP_AMOUNT];

		for (i, offset) in offsets.iter_mut().enumerate() {
			let chunk_idx = Self::access_pattern(OP_AMOUNT, i);
			*offset = (chunk_idx as usize * CHUNK_SIZE) as u32;
		}

		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;
		let file_name = "RNDWR.BIN";

		{
			let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

			let mut write_buf = [0u8; CHUNK_SIZE];
			Self::fill_buffer(&mut write_buf);

			let mut written_size = 0;

			while written_size < TOTAL_SIZE {
				file.write(&write_buf)?;
				written_size += CHUNK_SIZE;
			}

			file.flush()?;
			file.close()?;
		}

		let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteAppend)?;
		let mut buf = [0u8; CHUNK_SIZE];
		Self::fill_buffer(&mut buf);

		info!("Starting random raw write test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		//Test
		let start = processor::get_timer_ticks();

		for offset in offsets {
			file.seek_from_start(offset)?;
			file.write(&buf)?;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;
		let iops = OP_AMOUNT as f64 / elapsed_seconds;

		info!("Random write test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);
		info!("IOPS: {:.3}", iops);

		Ok(())
	}

	pub fn test_rnd_read_sdmmc(&mut self) -> Result<(), embedded_sdmmc::Error<VirtioBlkError>> {
		//Prep
		const TOTAL_SIZE: usize = 64 * 1024 * 1024; // MiB
		const CHUNK_SIZE: usize = 4 * 1024; // KiB chunks
		const OP_AMOUNT: usize = TOTAL_SIZE / CHUNK_SIZE;

		let total_size_mib = TOTAL_SIZE as f64 / 1024.0 / 1024.0;
		let chunk_size_kib = CHUNK_SIZE as f64 / 1024.0;

		let mut offsets = [0u32; OP_AMOUNT];

		for (i, offset) in offsets.iter_mut().enumerate() {
			let chunk_idx = Self::access_pattern(OP_AMOUNT, i);
			*offset = (chunk_idx as usize * CHUNK_SIZE) as u32;
		}

		let adapter = SdmmcBlkAdapter::new(self);
		let volume_mgr = VolumeManager::new(adapter, DummyTimeSource);

		let volume0 = volume_mgr.open_volume(VolumeIdx(0))?;
		let root_dir = volume0.open_root_dir()?;
		let file_name = "RNDRD.BIN";

		{
			let file = root_dir.open_file_in_dir(file_name, Mode::ReadWriteCreateOrTruncate)?;

			let mut write_buf = [0u8; CHUNK_SIZE];
			Self::fill_buffer(&mut write_buf);

			let mut written_size = 0;

			while written_size < TOTAL_SIZE {
				file.write(&write_buf)?;
				written_size += CHUNK_SIZE;
			}

			file.flush()?;
			file.close()?;
		}

		let file = root_dir.open_file_in_dir(file_name, Mode::ReadOnly)?;

		let mut read_buf = [0u8; CHUNK_SIZE];

		info!("Starting random sdmmc read test:");
		info!("Total size: {} MiB", total_size_mib);
		info!("Chunk size: {} KiB", chunk_size_kib);

		//Test
		let start = processor::get_timer_ticks();

		for offset in offsets {
			file.seek_from_start(offset)?;
			file.read(&mut read_buf)?;
		}

		let end = processor::get_timer_ticks();

		//Output
		let elapsed_seconds = (end - start) as f64 / 1_000_000.0;
		let throughput = total_size_mib / elapsed_seconds;
		let iops = OP_AMOUNT as f64 / elapsed_seconds;

		info!("Random sdmmc read test finished:");
		info!("Elapsed: {:.8} s", elapsed_seconds);
		info!("Throughput: {:.3} MiB/s", throughput);
		info!("IOPS: {:.3}", iops);

		Ok(())
	}

	//HELPER FUNC
	fn fill_buffer(buf: &mut [u8]) {
		for i in 0..buf.len() {
			buf[i] = (i % 256) as u8;
		}
	}

	fn fill_buffer_for_correctness(buf: &mut [u8], chunk_idx: usize) {
		for i in 0..buf.len() {
			buf[i] = ((chunk_idx + i) % 256) as u8;
		}
	}

	fn access_pattern(num_op: usize, idx: usize) -> u64 {
		((idx * 7159) % num_op) as u64
	}
}
