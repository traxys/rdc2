use bitflags::bitflags;
use bstr::{BStr, ByteSlice};

use super::FileSystem;

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

    pub(crate) fs: &'fs FileSystem<'device>,
}

impl<'fs, 'device> Inode<'fs, 'device> {
    pub(crate) unsafe fn from_fs(
        fs: &'fs FileSystem<'device>,
        inode: *mut InodeData,
    ) -> Inode<'fs, 'device> {
        Inode { data: inode, fs }
    }
    pub fn get_data(&self) -> *const InodeData {
        self.data
    }
    pub fn reader(&self) -> ReadInode<'_, 'fs, 'device> {
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
    pub fn read(&mut self, max_amount: Option<u32>) -> Option<(*mut u8, u32)> {
        unsafe {
            self.read_with(|input, remaining_in_block| {
                let amount = match max_amount {
                    Some(max_amount) => core::cmp::min(max_amount, remaining_in_block),
                    None => remaining_in_block,
                };
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
    pub(crate) unsafe fn from_ptr<'a>(inode: *mut u8) -> *mut InodeData {
        inode as *mut InodeData
    }
}
