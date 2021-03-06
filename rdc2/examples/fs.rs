use memmap::MmapOptions;
use simplelog::{Config, LevelFilter, TermLogger, TerminalMode};

use rdc2::{inode::EntryKind, inode::Permission, Ext2Device, FileSystem, Inode};

fn list(fs: &FileSystem<'_>, inode: &Inode<'_, '_>, tabs: usize) {
    if let Some(entries) = inode.get_dir_entries() {
        for entry in entries.skip(2) {
            for _ in 0..tabs {
                print!(" ");
            }
            match entry.kind {
                EntryKind::Directory => {
                    println!("{}:", entry.name);
                    if entry.name == "lost+found" {
                        continue;
                    }
                    if entry.name == "thing" {
                        let dir = fs.get_inode(entry.inode);
                        dir.create_inode_in_dir(
                            EntryKind::RegularFile,
                            Permission::all(),
                            0,
                            0,
                            "wtf_please".as_bytes(),
                        );
                    }
                    list(fs, &fs.get_inode(entry.inode), tabs + 4)
                }
                EntryKind::RegularFile => {
                    println!("file {}", entry.name);
                    let file = fs.get_inode(entry.inode);
                    if entry.name == "niche.txt" {
                        write_things(&file);
                        let mut writer = file.cursor().expect("niche.txt is not a file");
                        writer.advance(4);
                        writer.write("9\n".as_bytes());
                        let mut append = file.end().expect("niche.txt is not a file");
                        append.write("500\n".as_bytes());
                        dbg!(unsafe { &*file.get_data() });
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
fn read_to_end(inode: &Inode<'_, '_>, data: &mut Vec<u8>) {
    let mut reader = inode.cursor().expect("Is not a file");
    let mut buffer = [0; 128];
    loop {
        match reader.read(&mut buffer) {
            0 => break,
            n => data.extend_from_slice(&buffer[0..n]),
        }
    }
}
fn write_things(inode: &Inode<'_, '_>) {
    let mut writer = inode.cursor().expect("is not a file");
    for i in 0..500 {
        writer.write(format!("{}\n", i).as_bytes())
    }
}

fn main() {
    TermLogger::init(LevelFilter::Trace, Config::default(), TerminalMode::Mixed)
        .expect("no terminal");

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
