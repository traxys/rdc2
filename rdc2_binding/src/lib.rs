#![no_std]
use rdc2::{
    inode::{Cursor, DirectoryEntries, DirectoryEntry, EntryKind, Inode, InodeRef},
    Ext2Device, FileSystem,
};

trait OptionExt<T> {
    fn unwrap_write(self, location: *mut T) -> i64;
}
impl<T> OptionExt<T> for Option<T> {
    fn unwrap_write(self, location: *mut T) -> i64 {
        match self {
            None => -1,
            Some(val) => {
                unsafe { *location = val };
                0
            }
        }
    }
}

pub const ROOT_INODE: u32 = 2;

#[no_mangle]
pub unsafe extern "C" fn open<'device>(region: *mut u8) -> FileSystem<'device> {
    core::mem::transmute(Ext2Device::from_ptr(region).open())
}
#[no_mangle]
pub extern "C" fn fs_get_inode<'device, 'input>(
    fs: &'input FileSystem<'device>,
    inode: InodeRef,
) -> Inode<'input, 'device> {
    fs.get_inode(inode)
}

/// Write the Cursor in cursor_ptr if a Cursor can be created from this inode, and returns 0.
/// If a cursor can't be created, returns -1.
#[no_mangle]
pub extern "C" fn cursor<'inode, 'fs, 'device>(
    inode: &'inode Inode<'fs, 'device>,
    cursor_ptr: *mut Cursor<'inode, 'fs, 'device>,
) -> i64 {
    inode.cursor().unwrap_write(cursor_ptr)
}

/// See cursor, puts that cursor at the end of the file
#[no_mangle]
pub extern "C" fn cursor_at_end<'inode, 'fs, 'device>(
    inode: &'inode Inode<'fs, 'device>,
    cursor_ptr: *mut Cursor<'inode, 'fs, 'device>,
) -> i64 {
    inode.end().unwrap_write(cursor_ptr)
}
#[no_mangle]
pub extern "C" fn inode_size<'inode, 'fs, 'device>(inode: &'inode Inode<'fs, 'device>) -> u32 {
    inode.size()
}

/// See cursor, creates an iterator on the entries of this directory
#[no_mangle]
pub extern "C" fn directory_entries<'inode, 'fs, 'device>(
    inode: &'inode Inode<'fs, 'device>,
    entries: *mut DirectoryEntries<'inode, 'fs, 'device>,
) -> i64 {
    inode.get_dir_entries().unwrap_write(entries)
}

#[no_mangle]
pub unsafe extern "C" fn read<'inode, 'fs, 'device>(
    cursor: &mut Cursor<'inode, 'fs, 'device>,
    ptr: *mut u8,
    len: usize,
) -> usize {
    cursor.read(core::slice::from_raw_parts_mut(ptr, len))
}
#[no_mangle]
pub unsafe extern "C" fn write<'inode, 'fs, 'device>(
    cursor: &mut Cursor<'inode, 'fs, 'device>,
    ptr: *const u8,
    len: usize,
) {
    cursor.write(core::slice::from_raw_parts(ptr, len))
}

#[repr(C)]
pub struct RawDirEntry {
    pub inode: InodeRef,
    pub size: u16,
    pub kind: EntryKind,
    pub name_len: u8,
    pub name: *const u8,
}

#[no_mangle]
pub extern "C" fn read_next_entry<'inode, 'fs, 'device>(
    entries: &mut DirectoryEntries<'inode, 'fs, 'device>,
    entry: *mut RawDirEntry,
) -> i64 {
    entries
        .next()
        .map(|entry| RawDirEntry {
            inode: entry.inode,
            size: entry.size,
            kind: entry.kind,
            name: entry.name.as_ptr(),
            name_len: entry.name.len() as u8,
        })
        .unwrap_write(entry)
}
