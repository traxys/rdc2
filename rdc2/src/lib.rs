#![no_std]
extern crate core;

pub mod inode;
pub mod metadata;
pub use inode::{Inode, InodeRef};

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

        FileSystem {
            fs: self.device,
            block_size,
            superblock,
            extended,
            block_group_descriptor_table: unsafe {
                self.device.offset((block_size * block_table) as isize)
            } as *mut BlockGroupDescriptor,
            block_group_descriptor_table_len: number_of_groups,
        }
    }
}

/// The main way to interact with the filesystem
#[repr(C)]
pub struct FileSystem<'device> {
    fs: *mut u8,
    superblock: &'device mut Superblock,
    extended: &'device mut ExtendedSuperblock,

    block_group_descriptor_table: *mut BlockGroupDescriptor,
    block_group_descriptor_table_len: usize,
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
        unsafe {
            core::slice::from_raw_parts(
                self.block_group_descriptor_table,
                self.block_group_descriptor_table_len,
            )
        }
    }

    #[inline(always)]
    pub fn get_root(&self) -> Inode<'_, 'device> {
        self.get_inode(InodeRef(2))
    }

    fn reserve_bitmap(&self, start: *mut u8) -> u32 {
        let mut bitmap_block = start;
        let mut index = 0;

        while index < 1024 && unsafe { *bitmap_block } == 255 {
            index += 1;
            bitmap_block = unsafe { bitmap_block.offset(1) };
        }
        let byte = unsafe { *bitmap_block };
        log::trace!("Found space in bitmap at index {}: {:08b}", index, byte);

        let mut reserved_in_current = None;
        for i in 0..8 {
            if byte & (1 << i) == 0 {
                reserved_in_current = Some(i);
                break;
            }
        }
        let reserved_in_current = reserved_in_current.unwrap();

        log::trace!("Space is at index {}", reserved_in_current);
        unsafe { *bitmap_block |= 1 << reserved_in_current }

        let index = index * 8 + reserved_in_current;
        log::trace!("total index is {}", index);
        index
    }
    fn reserve_block(&self, group: u32) -> u32 {
        log::trace!("reserving new block in group {}", group);
        let bitmap =
            self.get_block_group_descriptor_table()[group as usize].block_address_of_block_bitmap;
        self.reserve_bitmap(unsafe { self.get_block(bitmap) })
    }
    fn reserve_inode(&self, group: u32) -> InodeRef {
        log::trace!("reserving new inode in group {}", group);
        let bitmap =
            self.get_block_group_descriptor_table()[group as usize].block_address_of_inode_bitmap;
        // Inodes start at 0
        InodeRef(self.reserve_bitmap(unsafe { self.get_block(bitmap) }) + 1)
    }

    pub fn get_inode(&self, inode: InodeRef) -> Inode<'_, 'device> {
        // I think it is safe because Inodes use *mut InodeData, you
        // can give multiple of them
        unsafe { Inode::from_fs(self, inode.0, self.get_inode_in_table(inode.0)) }
    }
    pub(crate) fn group_of_inode(&self, inode: InodeRef) -> u32 {
        (inode.0 - 1) / self.superblock.inode_count_in_group
    }

    /// This function assumes that you have exclusive access to that part of memory
    unsafe fn get_inode_in_table<'a>(&self, inode: u32) -> *mut InodeData {
        let block_group = self.group_of_inode(InodeRef(inode));
        let index = (inode - 1) % self.superblock.inode_count_in_group;

        let inode_table = self.get_block_group_descriptor_table()[block_group as usize]
            .starting_block_of_inode_table;

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
