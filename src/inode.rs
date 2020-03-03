use bitflags::bitflags;
use bstr::{BStr, ByteSlice};

use super::FileSystem;

/// A reference to an inode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct InodeRef(pub(crate) u32);

#[repr(C)]
struct RawDirectoryEntry {
    pub inode: InodeRef,
    pub size: u16,
    pub name_len: u8,
    pub kind: EntryKind,
}

#[derive(Debug)]
pub struct DirectoryEntry<'fs> {
    pub inode: InodeRef,
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
impl EntryKind {
    fn to_typeperm(&self) -> TypePermission {
        match self {
            EntryKind::Unkown => panic!("Unkown has no type"),
            EntryKind::RegularFile => TypePermission::REGULAR_FILE,
            EntryKind::Directory => TypePermission::DIR,
            EntryKind::CharDevice => TypePermission::CHAR_DEVICE,
            EntryKind::BlockDevice => TypePermission::BLOCK_DEVICE,
            EntryKind::Fifo => TypePermission::FIFO,
            EntryKind::Socket => TypePermission::UNIX_SOCKET,
            EntryKind::Symlink => TypePermission::SYMBOLIC_LINK,
        }
    }
}

pub struct Inode<'fs, 'device> {
    data: *mut InodeData,
    id: u32,
    group: u32,

    pub(crate) fs: &'fs FileSystem<'device>,
}

impl<'fs, 'device> Inode<'fs, 'device> {
    pub(crate) unsafe fn from_fs(
        fs: &'fs FileSystem<'device>,
        id: u32,
        inode: *mut InodeData,
    ) -> Inode<'fs, 'device> {
        let group = fs.group_of_inode(InodeRef(id));
        Inode {
            group,
            data: inode,
            fs,
            id,
        }
    }
    pub fn get_data(&self) -> *const InodeData {
        self.data
    }
    /*pub fn create_inode(
        &self,
        kind: EntryKind,
        perms: Permission,
        name: &BStr,
    ) -> Option<InodeRef> {
        let ty_perm = unsafe { (*self.data).type_permission };
        if ty_perm.contains(TypePermission::DIR) {
            let new_inode_ref = self.fs.reserve_inode(self.group);
            let inode = unsafe { self.fs.get_inode_in_table(new_inode_ref.0) };

            let entry = RawDirectoryEntry {
                inode: new_inode_ref,
                size: (core::mem::size_of::<RawDirectoryEntry>() + name.len()) as u16,
                name_len: name.len() as u8,
                kind,
            };

            unsafe {
                (*inode).type_permission = kind.to_typeperm() | perms.to_typeperm();
            }
            todo!()
        } else {
            None
        }
    }*/
    pub fn cursor(&self) -> Option<Cursor<'_, 'fs, 'device>> {
        let ty_perm = unsafe { (*self.data).type_permission };
        if ty_perm.contains(TypePermission::DIR) {
            None
        } else if ty_perm.contains(TypePermission::REGULAR_FILE) {
            Some(Cursor::new(self))
        } else {
            panic!("file type is unsuported, neither dir or file")
        }
    }
    pub fn end(&self) -> Option<Cursor<'_, 'fs, 'device>> {
        self.cursor().map(|mut cursor| {
            cursor.advance_to_end();
            cursor
        })
    }
    pub fn inode_ref(&self) -> InodeRef {
        InodeRef(self.id)
    }
    fn reserve_block(&self) -> u32 {
        let new_block = self.fs.reserve_block(self.group);
        for block in unsafe { &mut (*self.data).direct_block_pointers } {
            if *block == 0 {
                *block = new_block;
                break;
            }
        }
        new_block
    }
    pub fn get_dir_entries(&self) -> Option<DirectoryEntries<'_, 'fs, 'device>> {
        if !unsafe { (*self.data).type_permission }.contains(TypePermission::DIR) {
            None
        } else {
            log::trace!("reading entries for {}", self.id);
            Some(DirectoryEntries {
                reader: Cursor::new(self),
            })
        }
    }
}

struct InodeBlocks<'inode, 'fs, 'device> {
    remaining_blocks: u32,
    block_count: usize,
    single_count: usize,
    double_count: usize,
    triple_count: usize,
    inode: &'inode Inode<'fs, 'device>,
}

impl<'inode, 'fs, 'device> InodeBlocks<'inode, 'fs, 'device> {
    fn new(inode: &'inode Inode<'fs, 'device>) -> Self {
        let block_size = inode.fs.block_size as u32;
        let total_size = unsafe { (*inode.data).size_lower_32_bits };
        let remaining_blocks = if total_size % block_size == 0 {
            total_size / block_size
        } else {
            (total_size / block_size) + 1
        };
        InodeBlocks {
            remaining_blocks,
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
        if self.remaining_blocks > 0 {
            if self.block_count < 12 {
                //TODO: use the indirections
                let block = unsafe { (*self.inode.data).direct_block_pointers[self.block_count] };
                self.block_count += 1;
                self.remaining_blocks -= 1;

                if block == 0 {
                    panic!(
                        "Tried to read from block 0:\nRem: {}, Blocks: {:?}",
                        self.remaining_blocks,
                        unsafe { (*self.inode.data).direct_block_pointers }
                    );
                }
                log::trace!("next block for inode {} is {}", self.inode.id, block);
                Some(block)
            } else {
                panic!("Can't read in indirect blocks")
            }
        } else {
            None
        }
    }
}

pub struct Cursor<'inode, 'fs, 'device> {
    total_index: u32,
    total_remaining: u32,

    index_in_block: u32,
    remaining_in_block: u32,

    block_size: u32,
    allocated_blocks: InodeBlocks<'inode, 'fs, 'device>,
    current_head: Option<*mut u8>,
    inode: &'inode Inode<'fs, 'device>,
}
impl<'inode, 'fs, 'device> Cursor<'inode, 'fs, 'device> {
    fn new(inode: &'inode Inode<'fs, 'device>) -> Self {
        let block_size = inode.fs.block_size as u32;
        let mut blocks = InodeBlocks::new(inode);
        let current_block = blocks.next();
        let current_head = unsafe { current_block.map(|block| inode.fs.get_block(block)) };
        let total_size = unsafe { (*inode.data).size_lower_32_bits };
        Self {
            total_index: 0,
            total_remaining: total_size,

            index_in_block: 0,
            remaining_in_block: core::cmp::min(total_size, block_size),

            block_size,
            allocated_blocks: blocks,
            current_head,
            inode,
        }
    }
    pub fn advance(&mut self, mut count: u32) {
        while count > 0 {
            if self.index_in_block < self.block_size {
                let remaining_in_block = self.block_size - self.index_in_block;
                let advance_amount = core::cmp::min(remaining_in_block, count);
                count -= advance_amount;

                self.index_in_block += advance_amount;
                self.total_index += advance_amount;

                self.current_head = unsafe {
                    self.current_head
                        .map(|ptr| ptr.offset(advance_amount as isize))
                };
            } else {
                self.index_in_block = 0;
                match self.allocated_blocks.next() {
                    Some(b) => self.current_head = unsafe { Some(self.inode.fs.get_block(b)) },
                    None => return,
                }
            }
        }
    }
    pub fn advance_to_end(&mut self) {
        self.advance(unsafe { (*self.inode.data).size_lower_32_bits });
    }
    pub fn write(&mut self, data: &[u8]) {
        let mut current = 0;
        let mut head = match self.current_head {
            Some(h) => h,
            None => unsafe { self.inode.fs.get_block(self.inode.reserve_block()) },
        };
        while current < data.len() as u32 {
            if self.index_in_block < self.block_size {
                unsafe {
                    (*head) = data[current as usize];
                    self.current_head = Some(head.offset(1));
                    head = head.offset(1);
                }
                self.total_index += 1;
                self.index_in_block += 1;
                current += 1;
            } else {
                self.index_in_block = 0;
                match self.allocated_blocks.next() {
                    Some(b) => head = unsafe { self.inode.fs.get_block(b) },
                    None => head = unsafe { self.inode.fs.get_block(self.inode.reserve_block()) },
                }
                self.current_head = Some(head);
            }
        }
        let current_size = unsafe { (*self.inode.data).size_lower_32_bits };
        let new_size = core::cmp::max(current_size, self.total_index);
        unsafe { (*self.inode.data).size_lower_32_bits = new_size }
    }

    fn advance_block(&mut self) -> bool {
        let block = match self.allocated_blocks.next() {
            Some(b) => b,
            None => return false,
        };
        let block_size = self.inode.fs.block_size as u32;
        self.index_in_block = 0;
        self.remaining_in_block = core::cmp::min(self.total_remaining, block_size);
        log::trace!(
            "Setting {} as current_block in inode {} cursor",
            block,
            self.inode.id
        );
        self.current_head = unsafe { Some(self.inode.fs.get_block(block)) };
        true
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
        let current_block = self.current_head?;
        let (value, size) = reader(current_block, self.remaining_in_block);
        assert!(
            size <= self.remaining_in_block,
            "tried to read {} bytes, remaining_in_block was {}",
            size,
            self.remaining_in_block
        );
        log::trace!(
            "read a value of size {}. {} remaining in block",
            size,
            self.remaining_in_block
        );
        self.index_in_block += size;
        self.remaining_in_block -= size;
        self.current_head = Some(current_block.offset(size as isize));
        Some((value, size))
    }
    /// Reads up to max_amount bytes from the inode
    pub fn read(&mut self, max_amount: Option<u32>) -> Option<(*mut u8, u32)> {
        log::trace!(
            "reading {:?} at most from inode {}",
            max_amount,
            self.inode.id
        );
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
}

pub struct DirectoryEntries<'inode, 'fs, 'device> {
    reader: Cursor<'inode, 'fs, 'device>,
}

impl<'inode, 'fs, 'device> core::iter::Iterator for DirectoryEntries<'inode, 'fs, 'device> {
    type Item = DirectoryEntry<'fs>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let ((dir_entry, name), _) = self.reader.read_with(|input, _| {
                log::trace!("Reading directory entry from {:?}", input);
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
    pub struct Permission: u16 {
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
impl Permission {
    fn to_typeperm(&self) -> TypePermission {
        // Safety: just compare the two definitions
        unsafe { TypePermission::from_bits_unchecked(self.bits()) }
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
