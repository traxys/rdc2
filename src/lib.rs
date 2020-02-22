#![no_std]
extern crate core;

use bitflags::bitflags;
use bstr::{BStr, ByteSlice};

pub const SUPERBLOCK_SIZE: usize = 84;
pub const BLOCK_GROUP_DESCRITPOR_SIZE: usize = 32;
pub const EXTENDED_SUPERBLOCK_SIZE: usize = 1023 - SUPERBLOCK_SIZE;

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

    /// Safety: You must not get twice the same inode
    pub unsafe fn get_inode(&self, inode: u32) -> Inode<'_, 'device> {
        Inode {
            data: self.get_inode_in_table(inode),
            fs: self,
        }
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

        InodeData::from_ptr(inode_table_offset)
    }

    /// Safety: Don't have two handles on the same block !
    unsafe fn get_block(&self, index: u32) -> *mut u8 {
        self.fs.offset((self.block_size * index as usize) as isize)
    }
}

#[repr(C)]
struct RawDirectoryEntry {
    pub inode: u32,
    pub size: u16,
    pub name_len: u8,
    pub kind: EntryKind,
}

#[derive(Debug)]
pub struct DirectoryEntry<'fs> {
    pub inode: u32,
    pub size: u16,
    pub kind: EntryKind,
    pub name: &'fs BStr,
}

impl RawDirectoryEntry {
    unsafe fn from_ptr<'fs>(entry: *const u8) -> (*const RawDirectoryEntry, &'fs BStr) {
        let dir_entry = entry as *const RawDirectoryEntry;
        let name_start = entry.offset(core::mem::size_of::<RawDirectoryEntry>() as isize);
        let name_slice =
            core::slice::from_raw_parts(name_start, (*dir_entry).name_len as usize).as_bstr();
        (dir_entry, name_slice)
    }
}

impl<'fs> DirectoryEntry<'fs> {
    unsafe fn from_raw(
        dir_entry: *const RawDirectoryEntry,
        name: &'fs BStr,
    ) -> DirectoryEntry<'fs> {
        DirectoryEntry {
            inode: (*dir_entry).inode,
            kind: (*dir_entry).kind,
            size: (*dir_entry).size,
            name,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum EntryKind {
    Unkown = 0,
    RegularFile = 1,
    Directory = 2,
    CharDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    Symlink = 7,
}

pub struct Inode<'fs, 'device> {
    data: *mut InodeData,

    fs: &'fs FileSystem<'device>,
}

impl<'fs, 'device> Inode<'fs, 'device> {
    pub fn get_data(&self) -> *const InodeData {
        self.data
    }
    fn reader(&self) -> ReadInode<'_, 'fs, 'device> {
        ReadInode::new(self)
    }
    pub fn get_dir_entries(&self) -> Option<DirectoryEntries<'_, 'fs, 'device>> {
        if !unsafe { (*self.data).type_permission }.contains(TypePermission::DIR) {
            None
        } else {
            Some(DirectoryEntries {
                reader: self.reader(),
            })
        }
    }
}

struct InodeBlocks<'inode, 'fs, 'device> {
    block_count: usize,
    single_count: usize,
    double_count: usize,
    triple_count: usize,
    inode: &'inode Inode<'fs, 'device>,
}

impl<'inode, 'fs, 'device> InodeBlocks<'inode, 'fs, 'device> {
    fn new(inode: &'inode Inode<'fs, 'device>) -> Self {
        InodeBlocks {
            block_count: 0,
            single_count: 0,
            double_count: 0,
            triple_count: 0,
            inode,
        }
    }
}

impl<'inode, 'fs, 'device> core::iter::Iterator for InodeBlocks<'inode, 'fs, 'device> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.block_count < 12 {
            //TODO: use the indirections
            let block = unsafe { (*self.inode.data).direct_block_pointers[self.block_count] };
            self.block_count += 1;
            Some(block)
        } else {
            None
        }
    }
}

pub struct ReadInode<'inode, 'fs, 'device> {
    index_in_block: u32,
    remaining_in_block: u32,
    total_remaining: u32,
    blocks: InodeBlocks<'inode, 'fs, 'device>,
    current_block: u32,
    inode: &'inode Inode<'fs, 'device>,
}

impl<'inode, 'fs, 'device> ReadInode<'inode, 'fs, 'device> {
    fn new(inode: &'inode Inode<'fs, 'device>) -> Self {
        let total_size = unsafe { (*inode.data).size_lower_32_bits };
        let block_size = inode.fs.block_size as u32;
        let mut blocks = InodeBlocks::new(inode);
        let current_block = blocks.next().unwrap();
        ReadInode {
            index_in_block: 0,
            remaining_in_block: core::cmp::min(total_size, block_size),
            total_remaining: total_size,
            blocks,
            current_block,
            inode,
        }
    }
    fn advance_block(&mut self) -> bool {
        let block = match self.blocks.next() {
            Some(b) => b,
            None => return false,
        };
        let block_size = self.inode.fs.block_size as u32;
        self.index_in_block = 0;
        self.remaining_in_block = core::cmp::min(self.total_remaining, block_size);
        self.current_block = block;
        true
    }
    /// Reads up to max_amount bytes from the inode
    pub fn read(&mut self, max_amount: u32) -> Option<(*mut u8, u32)> {
        unsafe {
            self.read_with(|input, remaining_in_block| {
                let amount = core::cmp::min(max_amount, remaining_in_block);
                (input, amount)
            })
        }
    }
    /// Safety: You can't read more than a block boundry
    /// The function should take a pointer to the start of the region, and the maximum amount
    /// you can read
    pub unsafe fn read_with<T>(
        &mut self,
        reader: impl Fn(*mut u8, u32) -> (T, u32),
    ) -> Option<(T, u32)> {
        if self.remaining_in_block == 0 {
            if !self.advance_block() {
                return None;
            }
        }
        let ptr = self
            .inode
            .fs
            .get_block(self.current_block)
            .offset(self.index_in_block as isize);
        let (value, size) = reader(ptr, self.remaining_in_block);
        assert!(
            size <= self.remaining_in_block,
            "You should read less than what remains in the block"
        );
        self.index_in_block += size;
        self.remaining_in_block -= size;
        Some((value, size))
    }
}

pub struct DirectoryEntries<'inode, 'fs, 'device> {
    reader: ReadInode<'inode, 'fs, 'device>,
}

impl<'inode, 'fs, 'device> core::iter::Iterator for DirectoryEntries<'inode, 'fs, 'device> {
    type Item = DirectoryEntry<'fs>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let ((dir_entry, name), _) = self.reader.read_with(|input, _| {
                let (dir_entry, name) = RawDirectoryEntry::from_ptr(input);
                ((dir_entry, name), (*dir_entry).size as u32)
            })?;
            let entry = DirectoryEntry::from_raw(dir_entry, name);
            if entry.size == 0 {
                None
            } else {
                Some(entry)
            }
        }
    }
}

bitflags! {
    pub struct TypePermission: u16 {
        const FIFO = 0x1000;
        const CHAR_DEVICE = 0x2000;
        const DIR = 0x4000;
        const BLOCK_DEVICE = 0x6000;
        const REGULAR_FILE = 0x8000;
        const SYMBOLIC_LINK = 0xA000;
        const UNIX_SOCKET = 0xC000;

        const OTHER_EXECUTE = 0o00001;
        const OTHER_WRITE = 0o00002;
        const OTHER_READ = 0o00004;
        const GROUP_EXECUTE = 0o00010;
        const GROUP_WRITE = 0o00020;
        const GROUP_READ = 0o00040;
        const USER_EXECUTE = 0o00100;
        const USER_WRITE = 0o00200;
        const USER_READ = 0o00400;
        const STICKY_BIT = 0o01000;
        const SET_GROUP_ID = 0o02000;
        const SET_USER_ID = 0o04000;
    }
}
bitflags! {
    pub struct InodeFlags: u32 {
        const SECURE_DELETION = 0x00000001;
        const COPY_ON_DELETION = 0x00000002;
        const FILE_COMPRESSION = 0x00000004;
        const SYNCHRONOUS_UPDATES = 0x00000008;
        const IMMUTABLE_FILE = 0x00000010;
        const APPEND_ONLY = 0x00000020;
        const FILE_NOT_IN_DUMP = 0x00000040;
        const DONT_UPDATE_ACCESSED_TIME = 0x00000080;
        const HASH_INDEXED_DIR = 0x00010000;
        const AFS_DIR = 0x00020000;
        const JOURNAL_DATA = 0x00040000;
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct InodeData {
    pub type_permission: TypePermission,
    pub user_id: u16,
    pub size_lower_32_bits: u32,
    pub last_access_time: u32,
    pub creation_time: u32,
    pub last_modification_time: u32,
    pub deletion_time: u32,
    pub group_id: u16,
    pub hard_link_to_inode: u16,
    pub disk_sectors_used: u32,
    pub flags: InodeFlags,
    pub os_specific_one: u32,
    pub direct_block_pointers: [u32; 12],
    pub singly_indirect_block_pointer: u32,
    pub doubly_indirect_block_pointer: u32,
    pub triply_indirect_block_pointer: u32,
    pub generation_number: u32,
    pub acl: u32,
    pub upper_size_or_dir_acl: u32,
    pub block_address_of_fragment: u32,
    pub os_specific_two: [u8; 12],
}

impl InodeData {
    unsafe fn from_ptr<'a>(inode: *mut u8) -> *mut InodeData {
        inode as *mut InodeData
    }
}

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
    unsafe fn table_from_ptr<'a>(
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
    unsafe fn from_ptr<'a>(
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

#[derive(Debug)]
#[repr(transparent)]
pub struct UnsupportedFeatures(pub u32);

#[cfg(test)]
mod tests {
    extern crate std;
    use std::io::Read;

    use super::BlockGroupDescriptor;
    use super::ExtendedSuperblock;
    use super::Superblock;
    use super::BLOCK_GROUP_DESCRITPOR_SIZE;
    use super::EXTENDED_SUPERBLOCK_SIZE;
    use super::SUPERBLOCK_SIZE;

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
