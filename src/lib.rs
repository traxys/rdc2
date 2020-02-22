#![no_std]
extern crate core;

pub mod inode;
pub mod metadata;
pub use inode::Inode;

use inode::InodeData;
use metadata::{BlockGroupDescriptor, ExtendedSuperblock, Superblock};

/// A device partionned in ext2
pub struct Ext2Device {
    device: *mut u8,
}

impl Ext2Device {
    /// You give ownership of the fs to this. The pointer must be valid for as long as the
    /// Ext2Device exists
    pub unsafe fn from_ptr(device: *mut u8) -> Self {
        Ext2Device { device }
    }

    /// Open the filesystem
    pub fn open(&mut self) -> FileSystem<'_> {
        let (superblock, extended) = unsafe { Superblock::from_ptr(self.device.offset(1024)) };
        let extended = extended.expect("only support for version >= 1");

        let block_size = superblock.block_size();

        let mut number_of_groups = superblock.block_count / superblock.block_count_in_group;
        // If it does not divide evenly round up
        if superblock.block_count % superblock.block_count_in_group != 0 {
            number_of_groups += 1;
        }
        let number_of_groups = number_of_groups as usize;

        let block_table = if superblock.log_block_size == 0 { 2 } else { 1 };

        let block_group_descriptor_table = unsafe {
            BlockGroupDescriptor::table_from_ptr(
                // We look at block 1
                self.device.offset((block_size * block_table) as isize),
                number_of_groups,
            )
        };

        FileSystem {
            fs: self.device,
            block_size,
            superblock,
            extended,
            block_group_descriptor_table,
        }
    }
}

/// The main way to interact with the filesystem
pub struct FileSystem<'device> {
    fs: *mut u8,
    superblock: &'device mut Superblock,
    extended: &'device mut ExtendedSuperblock,

    block_group_descriptor_table: &'device mut [BlockGroupDescriptor],
    block_size: usize,
}

impl<'device> FileSystem<'device> {
    pub fn get_superblock(&self) -> &Superblock {
        self.superblock
    }
    pub fn get_extended_superblock(&self) -> &ExtendedSuperblock {
        self.extended
    }
    pub fn get_block_group_descriptor_table(&self) -> &[BlockGroupDescriptor] {
        self.block_group_descriptor_table
    }

    /// Safety: You must never hold two references to the root
    #[inline(always)]
    pub unsafe fn get_root(&self) -> Inode<'_, 'device> {
        self.get_inode(2)
    }

    // TODO: NewType InodeRef to be unable to give an incorrect one
    /// Safety: use a in bounds inode
    pub unsafe fn get_inode(&self, inode: u32) -> Inode<'_, 'device> {
        // I think it is safe because Inodes use *mut InodeData, you
        // can give multiple of them
        Inode::from_fs(self, self.get_inode_in_table(inode))
    }
    /// This function assumes that you have exclusive access to that part of memory
    unsafe fn get_inode_in_table<'a>(&self, inode: u32) -> *mut InodeData {
        let block_group = (inode - 1) / self.superblock.inode_count_in_group;
        let index = (inode - 1) % self.superblock.inode_count_in_group;

        let inode_table =
            self.block_group_descriptor_table[block_group as usize].starting_block_of_inode_table;

        let inode_table_offset = self
            .get_block(inode_table)
            .offset((self.extended.inode_struct_size as u32 * index) as isize);

        inode::InodeData::from_ptr(inode_table_offset)
    }

    /// Safety: Don't have two handles on the same block !
    unsafe fn get_block(&self, index: u32) -> *mut u8 {
        self.fs.offset((self.block_size * index as usize) as isize)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::io::Read;

    use super::Superblock;

    #[test]
    fn map_test_file() {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("test_fs")
            .unwrap();
        let mut backing = std::vec::Vec::with_capacity(500_000);
        file.read_to_end(&mut backing).unwrap();
        let ptr = backing.as_mut_ptr();

        let (superblock, _extended) = unsafe { Superblock::from_ptr(ptr.offset(1024)) };
        assert_eq!(superblock.inode_count, 56);
    }
}
