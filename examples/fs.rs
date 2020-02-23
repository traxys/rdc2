use memmap::MmapOptions;

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
                    list(fs, &fs.get_inode(entry.inode), tabs + 4)
                }
                EntryKind::RegularFile => {
                    println!("file {}", entry.name);
                    let file = fs.get_inode(entry.inode);
                    dbg!(unsafe { &*file.get_data() });
                    if entry.name == "foo" {
                        write_things(&file);
                    }

                    let mut content = Vec::new();
                    read_to_end(&file, &mut content);
                    for _ in 0..(tabs + 2) {
                        print!(" ");
                    }
                    println!("content: {}", String::from_utf8(content).unwrap());
                }
                k => println!("{:?} {}", k, entry.name),
            }
        }
    }
}
fn read_to_end(inode: &Inode<'_, '_>, buffer: &mut Vec<u8>) {
    let mut reader = inode.reader();
    while let Some((ptr, size)) = reader.read(None) {
        if size != 0 {
            let slice = unsafe { std::slice::from_raw_parts(ptr, size as usize) };
            buffer.extend_from_slice(slice);
        } else {
            break;
        }
    }
}
fn write_things(inode: &Inode<'_, '_>) {
    let mut writer = inode.writer();
    for i in 0..500 {
        writer.write(format!("{}\n", i).as_bytes())
    }
}

fn main() {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("test_fs")
        .unwrap();
    let mut device = unsafe { MmapOptions::new().map_mut(&file).unwrap() };
    let ptr = device.as_mut_ptr();

    let mut device = unsafe { Ext2Device::from_ptr(ptr) };
    let fs = device.open();
    dbg!(fs.get_superblock());
    dbg!(fs.get_extended_superblock());
    dbg!(fs.get_block_group_descriptor_table());
    let root = fs.get_root();
    dbg!(unsafe { &*root.get_data() });

    println!("/:");
    list(&fs, &root, 2);
}
