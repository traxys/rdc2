use bitflags::bitflags;
use bstr::{BStr, ByteSlice};

use super::FileSystem;
use core::convert::TryFrom;

/// A reference to an inode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct InodeRef(pub(crate) u32);

#[derive(Debug)]
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
    unsafe fn from_ptr_mut<'fs>(entry: *mut u8) -> (*mut RawDirectoryEntry, &'fs BStr) {
        let dir_entry = entry as *mut RawDirectoryEntry;
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

    pub fn create_inode_in_dir(
        &self,
        kind: EntryKind,
        perms: Permission,
        user_id: u16,
        group_id: u16,
        name: &[u8],
    ) -> Option<InodeRef> {
        if let EntryKind::Directory = kind {
            unimplemented!("Can't create a directory")
        }
        let ty_perm = unsafe { (*self.data).type_permission };
        if ty_perm.contains(TypePermission::DIR) {
            let new_inode_ref = self.fs.reserve_inode(self.group);
            log::trace!(
                "Assigning inode {:?} (name: {})",
                new_inode_ref,
                name.as_bstr()
            );
            let inode = unsafe { self.fs.get_inode_in_table(new_inode_ref.0) };

            let mut entries = DirectoryEntries {
                reader: Cursor::new(self),
            };
            entries.add_entry(kind, name, new_inode_ref);

            unsafe {
                (*inode).type_permission = kind.to_typeperm() | perms.to_typeperm();
                (*inode).hard_link_to_inode = 1;
                (*inode).user_id = user_id;
                (*inode).group_id = group_id;
            }
            Some(new_inode_ref)
        } else {
            None
        }
    }
    pub fn cursor(&self) -> Option<Cursor<'_, 'fs, 'device>> {
        let ty_perm = unsafe { (*self.data).type_permission };
        log::trace!("Getting cursor on inode {}, perms: {:?}", self.id, ty_perm);
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
        log::trace!("Getting entries on inode {}", self.id);
        if !unsafe { (*self.data).type_permission }.contains(TypePermission::DIR) {
            None
        } else {
            log::trace!("reading entries for {}", self.id);
            Some(DirectoryEntries {
                reader: Cursor::new(self),
            })
        }
    }
    pub fn size(&self) -> u32 {
        unsafe { (*self.data).size_lower_32_bits }
    }
}

pub struct Cursor<'inode, 'fs, 'device> {
    inode: &'inode Inode<'fs, 'device>,

    total_index: u32,
    block_size: u32,
}
impl<'inode, 'fs, 'device> Cursor<'inode, 'fs, 'device> {
    fn new(inode: &'inode Inode<'fs, 'device>) -> Self {
        Self {
            inode,
            total_index: 0,
            block_size: inode.fs.block_size as u32,
        }
    }
    /// This returns a ptr aligned to the start of the place you want
    /// to do something on, with the maximum bytes available
    #[inline]
    fn get_ptr(&self) -> Option<(*mut u8, u32)> {
        let block_ptr = unsafe { self.inode.fs.get_block(self.get_current_block_index()?) };
        let index_in_block = self.total_index % self.block_size;
        Some((
            unsafe { block_ptr.offset(index_in_block as isize) },
            self.block_size - index_in_block,
        ))
    }
    #[inline]
    fn remain_in_block(&self) -> u32 {
        self.block_size - (self.total_index % self.block_size)
    }
    #[inline]
    fn get_current_block_index(&self) -> Option<u32> {
        let block_count = self.total_index / self.block_size;
        if block_count > 12 {
            panic!("Can't use more than 12 blocks");
        } else {
            match unsafe { (*self.inode.get_data()).direct_block_pointers[block_count as usize] } {
                0 => None,
                b => {
                    log::trace!("Got ptr the block index {} for inode {}", b, self.inode.id);
                    Some(b)
                }
            }
        }
    }
    #[inline]
    unsafe fn peek_access_with<T>(
        &self,
        f: impl Fn(*mut u8, u32) -> Option<(T, u32)>,
    ) -> Option<(T, u32)> {
        let (current_position, remain) = self.get_ptr()?;
        f(current_position, remain)
    }
    #[inline]
    unsafe fn access_with<T>(
        &mut self,
        f: impl Fn(*mut u8, u32) -> Option<(T, u32)>,
    ) -> Option<(T, u32)> {
        match self.peek_access_with(f) {
            None => None,
            Some((ptr, read)) => {
                self.total_index += read;
                Some((ptr, read))
            }
        }
    }
    #[inline]
    pub unsafe fn peek_with<T>(
        &self,
        f: impl Fn(*const u8, u32) -> Option<(T, u32)>,
    ) -> Option<(T, u32)> {
        let (current_position, remain) = self.get_ptr()?;
        f(current_position, remain)
    }
    #[inline]
    pub unsafe fn read_with<T>(
        &mut self,
        f: impl Fn(*const u8, u32) -> Option<(T, u32)>,
    ) -> Option<(T, u32)> {
        match self.peek_with(f) {
            None => None,
            Some((ptr, read)) => {
                self.total_index += read;
                Some((ptr, read))
            }
        }
    }
    fn read_to_end_of_block_at_most(&mut self, buffer: &mut [u8]) -> Option<u32> {
        let (ptr, remain) = self.get_ptr()?;

        let read_amount = core::cmp::min(remain, buffer.len() as u32);
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, buffer.as_mut_ptr(), read_amount as usize);
        }

        self.total_index += read_amount;
        Some(read_amount)
    }
    #[inline]
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        let mut index = 0;
        log::trace!("Reading at most {} bytes from inode {}", buffer.len(), self.inode.id);
        while index < buffer.len() {
            match self.read_to_end_of_block_at_most(&mut buffer[index..]) {
                None => break,
                Some(read_amount) => index += read_amount as usize,
            }
        }
        index
    }
    fn allocate_new_block(&mut self) -> *mut u8 {
        let new_block_index = self.inode.reserve_block();
        unsafe { self.inode.fs.get_block(new_block_index) }
    }
    fn write_to_end_of_block_at_most(&mut self, data: &[u8]) -> u32 {
        let (ptr, remain) = self
            .get_ptr()
            .unwrap_or_else(|| (self.allocate_new_block(), self.block_size));
        let write_amount = core::cmp::min(remain, data.len() as u32);

        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, write_amount as usize);
        }

        self.total_index += write_amount;
        write_amount
    }
    #[inline]
    pub fn write(&mut self, data: &[u8]) {
        let mut index = 0;
        while index < data.len() {
            index += self.write_to_end_of_block_at_most(&data[index..]) as usize;
        }
    }
    #[inline]
    pub fn advance(&mut self, amount: u32) {
        // You should not advance to outside what is currently defined
        self.total_index += core::cmp::min(amount, self.inode.size());
    }
    #[inline]
    pub fn advance_to_end(&mut self) {
        self.advance(self.inode.size() - self.total_index)
    }
    fn align(&mut self, align_to: u32) -> Option<u32> {
        let index_in_block = self.total_index % self.block_size;
        let misalign = index_in_block % align_to;
        if misalign > self.remain_in_block() {
            None
        } else {
            Some(misalign)
        }
    }
}

pub struct DirectoryEntries<'inode, 'fs, 'device> {
    reader: Cursor<'inode, 'fs, 'device>,
}

impl<'inode, 'fs, 'device> DirectoryEntries<'inode, 'fs, 'device> {
    /// Make sure thant name.len() < 255
    fn add_entry(&mut self, kind: EntryKind, name: &[u8], inode: InodeRef) {
        let new_entry_size = (name.len() + core::mem::size_of::<RawDirectoryEntry>()) as u16;
        loop {
            match unsafe { self.peek() } {
                None => todo!("No peeking in entries"),
                Some((dir_entry, split_name)) => {
                    let padding_size: u16 = unsafe {
                        (*dir_entry).size
                            - (u16::from((*dir_entry).name_len)
                                + core::mem::size_of::<RawDirectoryEntry>() as u16)
                    };
                    // We don't have the space to insert our entry, let's try the next one
                    if padding_size < new_entry_size {
                        log::trace!("Skipping {}, only has {} padding", split_name, padding_size);
                        self.next();
                        continue;
                    }
                    match self.reader.align(4) {
                        // We are in the next block, we do what we must
                        None => todo!("Writing to next block for dir entries"),
                        Some(correction) => {
                            log::trace!("Trying {}, checking align", split_name);
                            let remaining_padding = padding_size - correction as u16;
                            if remaining_padding < new_entry_size {
                                log::trace!("Skipping {}, only has {} padding after align (corrected by {} bytes)", split_name, remaining_padding, correction);
                                self.next();
                                continue;
                            } else {
                                // We can now change the length of the current entry to leave space
                                // for ours
                                // The space of the new entry is (*dir_entry).size + correction
                                log::trace!("Splitting {} to write new entry", split_name);
                                unsafe {
                                    (*dir_entry).size = correction as u16
                                        + u16::try_from(split_name.len())
                                            .expect("name was not an u16")
                                        + core::mem::size_of::<RawDirectoryEntry>() as u16;
                                    self.reader.advance((*dir_entry).size as u32);
                                };
                                let new_raw_entry = RawDirectoryEntry {
                                    inode,
                                    size: remaining_padding,
                                    name_len: u8::try_from(name.len())
                                        .expect("name was more than 255"),
                                    kind,
                                };
                                unsafe {
                                    self.write_dir_entry(new_raw_entry, name);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    unsafe fn write_dir_entry(&mut self, entry: RawDirectoryEntry, name: &[u8]) {
        self.reader.write(core::slice::from_raw_parts(
            &entry as *const RawDirectoryEntry as *const u8,
            core::mem::size_of::<RawDirectoryEntry>(),
        ));
        self.reader.write(name);
    }

    unsafe fn peek(&self) -> Option<(*mut RawDirectoryEntry, &'fs BStr)> {
        let ((dir_entry, name), _) = self.reader.peek_access_with(|input, remain| {
            if remain < core::mem::size_of::<RawDirectoryEntry>() as u32 {
                None
            } else {
                Some(DirectoryEntries::read_raw_entry(input))
            }
        })?;
        if (*dir_entry).size == 0 {
            None
        } else {
            Some((dir_entry, name))
        }
    }

    unsafe fn read_raw_entry(start: *mut u8) -> ((*mut RawDirectoryEntry, &'fs BStr), u32) {
        log::trace!("Reading directory entry from {:?}", start);
        let (dir_entry, name) = RawDirectoryEntry::from_ptr_mut(start);
        log::trace!("name {:?}, entry", name);
        ((dir_entry, name), (*dir_entry).size as u32)
    }
}

impl<'inode, 'fs, 'device> core::iter::Iterator for DirectoryEntries<'inode, 'fs, 'device> {
    type Item = DirectoryEntry<'fs>;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let ((dir_entry, name), _) = self.reader.access_with(|input, remain| {
                if remain < core::mem::size_of::<RawDirectoryEntry>() as u32 {
                    None
                } else {
                    Some(DirectoryEntries::read_raw_entry(input))
                }
            })?;

            log::trace!("Reading raw entry {:?}", *dir_entry);
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
