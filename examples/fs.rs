use std::io::Read;

use rdc2::{inode::EntryKind, Ext2Device, FileSystem, Inode};

fn list(fs: &FileSystem<'_>, inode: &Inode<'_, '_>, tabs: usize) {
    if let Some(entries) = inode.get_dir_entries() {
        for entry in entries.skip(2) {
            for _ in 0..tabs {
                print!(" ");
            }
            match entry.kind {
                EntryKind::Directory => {
                    println!("{}:", entry.name);
                    list(fs, unsafe { &fs.get_inode(entry.inode) }, tabs + 4)
                }
                k => println!("{:?} {}", k, entry.name),
            }
        }
    }
}

fn main() {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("test_fs")
        .unwrap();
    let mut device = Vec::with_capacity(1_000_000);
    file.read_to_end(&mut device).unwrap();

    let ptr = device.as_mut_ptr();

    let mut device = unsafe { Ext2Device::from_ptr(ptr) };
    let fs = device.open();
    dbg!(fs.get_superblock());
    dbg!(fs.get_extended_superblock());
    dbg!(fs.get_block_group_descriptor_table());
    let root = unsafe { fs.get_root() };
    dbg!(unsafe { &*root.get_data() });

    println!("/:");
    list(&fs, &root, 2);
}
