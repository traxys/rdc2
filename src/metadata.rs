pub const SUPERBLOCK_SIZE: usize = 84;
pub const BLOCK_GROUP_DESCRITPOR_SIZE: usize = 32;
pub const EXTENDED_SUPERBLOCK_SIZE: usize = 1023 - SUPERBLOCK_SIZE;

use bitflags::bitflags;

#[repr(C)]
pub struct BlockGroupDescriptor {
    pub block_address_of_block_bitmap: u32,
    pub block_address_of_inode_bitmap: u32,
    pub starting_block_of_inode_table: u32,
    pub unallocated_blocks_in_group: u16,
    pub unallocated_inodes_in_group: u16,
    pub number_of_directories_in_group: u16,
    _unused: [u8; 14],
}

impl BlockGroupDescriptor {
    pub(crate) unsafe fn table_from_ptr<'a>(
        start: *mut u8,
        number_of_groups: usize,
    ) -> &'a mut [BlockGroupDescriptor] {
        core::slice::from_raw_parts_mut(start as *mut BlockGroupDescriptor, number_of_groups)
    }
}
impl core::fmt::Debug for BlockGroupDescriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockGroupDescriptor")
            .field(
                "block_address_of_block_bitmap",
                &self.block_address_of_block_bitmap,
            )
            .field(
                "block_address_of_inode_bitmap",
                &self.block_address_of_inode_bitmap,
            )
            .field(
                "starting_block_of_inode_table",
                &self.starting_block_of_inode_table,
            )
            .field(
                "unallocated_blocks_in_group",
                &self.unallocated_blocks_in_group,
            )
            .field(
                "unallocated_inodes_in_group",
                &self.unallocated_inodes_in_group,
            )
            .field(
                "number_of_directories_in_group",
                &self.number_of_directories_in_group,
            )
            .finish()
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Superblock {
    pub inode_count: u32,
    pub block_count: u32,
    pub block_superuser: u32,
    pub unallocated_blocks: u32,
    pub unallocated_inodes: u32,
    pub index_of_superblock: u32,
    pub log_block_size: u32,
    pub log_fragment_size: u32,
    pub block_count_in_group: u32,
    pub fragment_count_in_group: u32,
    pub inode_count_in_group: u32,
    pub last_mounted: u32,
    pub last_written: u32,
    pub number_of_times_mounted_since_last_consitency_check: u16,
    pub number_of_mounts_until_consistency_check: u16,
    pub ext2sig: u16,
    pub state: FsState,
    pub on_error: OnError,
    pub minor_version: u16,
    pub time_since_last_constiency_check: u32,
    pub time_between_forced_consistency_check: u32,
    pub creator_system_id: OsId,
    pub major_version: u32,
    pub user_id_allowed_to_reserve: u16,
    pub group_id_allowed_to_reserve: u16,
}

impl Superblock {
    /// You must provide a valid superblock start.
    /// You must *NOT* use the locations from start upto start + 1023
    /// The lifetime 'a is the lifetime of the pointer
    pub(crate) unsafe fn from_ptr<'a>(
        start: *mut u8,
    ) -> (&'a mut Superblock, Option<&'a mut ExtendedSuperblock>) {
        let superblock = (start as *mut Superblock)
            .as_mut()
            .expect("Superblock was null");

        assert_eq!(superblock.ext2sig, 0xef53, "Ext2 is not valid");

        let extended = if superblock.major_version > 1 {
            None
        } else {
            Some(
                match (start.offset(SUPERBLOCK_SIZE as isize) as *mut ExtendedSuperblock).as_mut() {
                    Some(p) => p,
                    None => core::hint::unreachable_unchecked(),
                },
            )
        };

        (superblock, extended)
    }

    pub fn block_size(&mut self) -> usize {
        1024 << self.log_block_size
    }
}

#[repr(u16)]
#[derive(Debug)]
pub enum FsState {
    Clean = 1,
    Errored = 2,
}

#[repr(u16)]
#[derive(Debug)]
pub enum OnError {
    Ignore = 1,
    RemountReadOnly = 2,
    KernelPanic = 3,
}

#[repr(u16)]
#[derive(Debug)]
pub enum OsId {
    Linux = 0,
    GnuHurd = 1,
    Masix = 2,
    FreeBSD = 3,
    OtherLite = 4,
}

/// bytes 236 to 1023 are not counted
#[repr(C)]
pub struct ExtendedSuperblock {
    pub first_non_reserved_inode: u32,
    pub inode_struct_size: u16,
    pub part_of_block: u16,
    pub optional_features: OptionalFeatures,
    pub required_features: RequiredFeatures,
    pub write_features: WriteFeatures,
    pub fs_id: Id,
    pub volume_name: [i8; 16],
    pub path_last_mounted_at: [i8; 64],
    pub compression_algorithm: u32,
    pub number_of_blocks_to_preallocate_files: u8,
    pub number_of_blocks_to_preallocate_dirs: u8,
    pub unused: u16,
    pub journal_id: Id,
    pub journal_inode: u32,
    pub journal_device: u32,
    pub head_of_orphan_list: u32,
}

impl core::fmt::Debug for ExtendedSuperblock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExtendedSuperblock")
            .field("first_non_reserved_inode", &self.first_non_reserved_inode)
            .field("inode_struct_size", &self.inode_struct_size)
            .field("part_of_block", &self.part_of_block)
            .field("optional_features", &self.optional_features)
            .field("required_features", &self.required_features)
            .field("write_features", &self.write_features)
            .field("fs_id", &self.fs_id)
            .field("volume_name", unsafe {
                &cstr_core::CStr::from_ptr(&self.volume_name as *const cstr_core::c_char)
            })
            .field("path_last_mounted_at", unsafe {
                &cstr_core::CStr::from_ptr(&self.path_last_mounted_at as *const cstr_core::c_char)
            })
            .field("compression_algorithm", &self.compression_algorithm)
            .field(
                "number_of_blocks_to_preallocate_files",
                &self.number_of_blocks_to_preallocate_files,
            )
            .field(
                "number_of_blocks_to_preallocate_dirs",
                &self.number_of_blocks_to_preallocate_dirs,
            )
            .field("journal_id", &self.journal_id)
            .field("journal_inode", &self.journal_inode)
            .field("journal_device", &self.journal_device)
            .field("head_of_orphan_list", &self.head_of_orphan_list)
            .finish()
    }
}

#[repr(transparent)]
pub struct Id(pub [u8; 16]);

impl core::fmt::Debug for Id {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?
        }
        Ok(())
    }
}

bitflags! {
    pub struct OptionalFeatures: u32 {
        const PREALLOCATE = 0x0001;
        const AFS_SERVER = 0x0002;
        const JOURNALING = 0x0004;
        const EXTENDED_INODES = 0x0008;
        const RESIZEABLE = 0x0010;
        const DIR_HASH_INDEX = 0x0020;
    }
}

bitflags! {
    pub struct RequiredFeatures: u32 {
        const COMPRESSION = 0x0001;
        const TYPED_DIRECTORY = 0x0002;
        const REPLAY_JOURNAL = 0x0004;
        const JOURNAL = 0x0008;
    }
}

bitflags! {
    pub struct WriteFeatures: u32 {
        const SPARSE_SUPERBLOCK_GROUP_DESCRIPTOR_TABLE = 0x0001;
        const FILE_SIZE_64 = 0x0002;
        const BINARY_TREE_DIRECTORY = 0x0004;
    }
}

#[cfg(test)]
mod tests {
    use super::BlockGroupDescriptor;
    use super::ExtendedSuperblock;
    use super::Superblock;
    use super::BLOCK_GROUP_DESCRITPOR_SIZE;
    use super::EXTENDED_SUPERBLOCK_SIZE;
    use super::SUPERBLOCK_SIZE;


    #[test]
    fn block_descriptor_size() {
        assert_eq!(
            core::mem::size_of::<BlockGroupDescriptor>(),
            BLOCK_GROUP_DESCRITPOR_SIZE
        )
    }

    #[test]
    fn superblock_size() {
        assert_eq!(core::mem::size_of::<Superblock>(), SUPERBLOCK_SIZE)
    }
    #[test]
    fn extended_superblock_size() {
        assert_eq!(
            core::mem::size_of::<ExtendedSuperblock>() + 1023 - 236,
            EXTENDED_SUPERBLOCK_SIZE
        )
    }
}
