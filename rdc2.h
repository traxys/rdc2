#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define ROOT_INODE 2

enum EntryKind {
  Unkown = 0,
  RegularFile = 1,
  Directory = 2,
  CharDevice = 3,
  BlockDevice = 4,
  Fifo = 5,
  Socket = 6,
  Symlink = 7,
};
typedef uint8_t EntryKind;

enum FsState {
  Clean = 1,
  Errored = 2,
};
typedef uint16_t FsState;

enum OnError {
  Ignore = 1,
  RemountReadOnly = 2,
  KernelPanic = 3,
};
typedef uint16_t OnError;

enum OsId {
  Linux = 0,
  GnuHurd = 1,
  Masix = 2,
  FreeBSD = 3,
  OtherLite = 4,
};
typedef uint16_t OsId;

struct TypePermission {
  uint16_t bits;
};
#define TypePermission_FIFO (TypePermission){ .bits = 4096 }
#define TypePermission_CHAR_DEVICE (TypePermission){ .bits = 8192 }
#define TypePermission_DIR (TypePermission){ .bits = 16384 }
#define TypePermission_BLOCK_DEVICE (TypePermission){ .bits = 24576 }
#define TypePermission_REGULAR_FILE (TypePermission){ .bits = 32768 }
#define TypePermission_SYMBOLIC_LINK (TypePermission){ .bits = 40960 }
#define TypePermission_UNIX_SOCKET (TypePermission){ .bits = 49152 }
#define TypePermission_OTHER_EXECUTE (TypePermission){ .bits = 1 }
#define TypePermission_OTHER_WRITE (TypePermission){ .bits = 2 }
#define TypePermission_OTHER_READ (TypePermission){ .bits = 4 }
#define TypePermission_GROUP_EXECUTE (TypePermission){ .bits = 8 }
#define TypePermission_GROUP_WRITE (TypePermission){ .bits = 16 }
#define TypePermission_GROUP_READ (TypePermission){ .bits = 32 }
#define TypePermission_USER_EXECUTE (TypePermission){ .bits = 64 }
#define TypePermission_USER_WRITE (TypePermission){ .bits = 128 }
#define TypePermission_USER_READ (TypePermission){ .bits = 256 }
#define TypePermission_STICKY_BIT (TypePermission){ .bits = 512 }
#define TypePermission_SET_GROUP_ID (TypePermission){ .bits = 1024 }
#define TypePermission_SET_USER_ID (TypePermission){ .bits = 2048 }

struct InodeFlags {
  uint32_t bits;
};
#define InodeFlags_SECURE_DELETION (InodeFlags){ .bits = 1 }
#define InodeFlags_COPY_ON_DELETION (InodeFlags){ .bits = 2 }
#define InodeFlags_FILE_COMPRESSION (InodeFlags){ .bits = 4 }
#define InodeFlags_SYNCHRONOUS_UPDATES (InodeFlags){ .bits = 8 }
#define InodeFlags_IMMUTABLE_FILE (InodeFlags){ .bits = 16 }
#define InodeFlags_APPEND_ONLY (InodeFlags){ .bits = 32 }
#define InodeFlags_FILE_NOT_IN_DUMP (InodeFlags){ .bits = 64 }
#define InodeFlags_DONT_UPDATE_ACCESSED_TIME (InodeFlags){ .bits = 128 }
#define InodeFlags_HASH_INDEXED_DIR (InodeFlags){ .bits = 65536 }
#define InodeFlags_AFS_DIR (InodeFlags){ .bits = 131072 }
#define InodeFlags_JOURNAL_DATA (InodeFlags){ .bits = 262144 }

struct InodeData {
  struct TypePermission type_permission;
  uint16_t user_id;
  uint32_t size_lower_32_bits;
  uint32_t last_access_time;
  uint32_t creation_time;
  uint32_t last_modification_time;
  uint32_t deletion_time;
  uint16_t group_id;
  uint16_t hard_link_to_inode;
  uint32_t disk_sectors_used;
  struct InodeFlags flags;
  uint32_t os_specific_one;
  uint32_t direct_block_pointers[12];
  uint32_t singly_indirect_block_pointer;
  uint32_t doubly_indirect_block_pointer;
  uint32_t triply_indirect_block_pointer;
  uint32_t generation_number;
  uint32_t acl;
  uint32_t upper_size_or_dir_acl;
  uint32_t block_address_of_fragment;
  uint8_t os_specific_two[12];
};

struct Superblock {
  uint32_t inode_count;
  uint32_t block_count;
  uint32_t block_superuser;
  uint32_t unallocated_blocks;
  uint32_t unallocated_inodes;
  uint32_t index_of_superblock;
  uint32_t log_block_size;
  uint32_t log_fragment_size;
  uint32_t block_count_in_group;
  uint32_t fragment_count_in_group;
  uint32_t inode_count_in_group;
  uint32_t last_mounted;
  uint32_t last_written;
  uint16_t number_of_times_mounted_since_last_consitency_check;
  uint16_t number_of_mounts_until_consistency_check;
  uint16_t ext2sig;
  FsState state;
  OnError on_error;
  uint16_t minor_version;
  uint32_t time_since_last_constiency_check;
  uint32_t time_between_forced_consistency_check;
  OsId creator_system_id;
  uint32_t major_version;
  uint16_t user_id_allowed_to_reserve;
  uint16_t group_id_allowed_to_reserve;
};

struct OptionalFeatures {
  uint32_t bits;
};
#define OptionalFeatures_PREALLOCATE (OptionalFeatures){ .bits = 1 }
#define OptionalFeatures_AFS_SERVER (OptionalFeatures){ .bits = 2 }
#define OptionalFeatures_JOURNALING (OptionalFeatures){ .bits = 4 }
#define OptionalFeatures_EXTENDED_INODES (OptionalFeatures){ .bits = 8 }
#define OptionalFeatures_RESIZEABLE (OptionalFeatures){ .bits = 16 }
#define OptionalFeatures_DIR_HASH_INDEX (OptionalFeatures){ .bits = 32 }

struct RequiredFeatures {
  uint32_t bits;
};
#define RequiredFeatures_COMPRESSION (RequiredFeatures){ .bits = 1 }
#define RequiredFeatures_TYPED_DIRECTORY (RequiredFeatures){ .bits = 2 }
#define RequiredFeatures_REPLAY_JOURNAL (RequiredFeatures){ .bits = 4 }
#define RequiredFeatures_JOURNAL (RequiredFeatures){ .bits = 8 }

struct WriteFeatures {
  uint32_t bits;
};
#define WriteFeatures_SPARSE_SUPERBLOCK_GROUP_DESCRIPTOR_TABLE (WriteFeatures){ .bits = 1 }
#define WriteFeatures_FILE_SIZE_64 (WriteFeatures){ .bits = 2 }
#define WriteFeatures_BINARY_TREE_DIRECTORY (WriteFeatures){ .bits = 4 }

typedef uint8_t Id[16];

/**
 * bytes 236 to 1023 are not counted
 */
struct ExtendedSuperblock {
  uint32_t first_non_reserved_inode;
  uint16_t inode_struct_size;
  uint16_t part_of_block;
  struct OptionalFeatures optional_features;
  struct RequiredFeatures required_features;
  struct WriteFeatures write_features;
  Id fs_id;
  int8_t volume_name[16];
  int8_t path_last_mounted_at[64];
  uint32_t compression_algorithm;
  uint8_t number_of_blocks_to_preallocate_files;
  uint8_t number_of_blocks_to_preallocate_dirs;
  uint16_t unused;
  Id journal_id;
  uint32_t journal_inode;
  uint32_t journal_device;
  uint32_t head_of_orphan_list;
};

struct BlockGroupDescriptor {
  uint32_t block_address_of_block_bitmap;
  uint32_t block_address_of_inode_bitmap;
  uint32_t starting_block_of_inode_table;
  uint16_t unallocated_blocks_in_group;
  uint16_t unallocated_inodes_in_group;
  uint16_t number_of_directories_in_group;
  uint8_t _unused[14];
};

/**
 * The main way to interact with the filesystem
 */
struct FileSystem {
  uint8_t *fs;
  struct Superblock *superblock;
  struct ExtendedSuperblock *extended;
  struct BlockGroupDescriptor *block_group_descriptor_table;
  uintptr_t block_group_descriptor_table_len;
  uintptr_t block_size;
};

struct Inode {
  struct InodeData *data;
  const struct FileSystem *fs;
  uint32_t id;
  uint32_t group;
};

struct Cursor {
  const struct Inode *inode;
  uint32_t total_index;
  uint32_t block_size;
};

struct DirectoryEntries {
  struct Cursor reader;
};

/**
 * A reference to an inode
 */
typedef uint32_t InodeRef;

struct RawDirEntry {
  InodeRef inode;
  uint16_t size;
  EntryKind kind;
  uint8_t name_len;
  const uint8_t *name;
};

/**
 * Write the Cursor in cursor_ptr if a Cursor can be created from this inode, and returns 0.
 * If a cursor can't be created, returns -1.
 */
int64_t cursor(const struct Inode *inode, struct Cursor *cursor_ptr);

/**
 * See cursor, puts that cursor at the end of the file
 */
int64_t cursor_at_end(const struct Inode *inode, struct Cursor *cursor_ptr);

/**
 * See cursor, creates an iterator on the entries of this directory
 */
int64_t directory_entries(const struct Inode *inode, struct DirectoryEntries *entries);

struct Inode fs_get_inode(const struct FileSystem *fs, InodeRef inode);

uint32_t inode_size(const struct Inode *inode);

struct FileSystem open(uint8_t *region);

uintptr_t read(struct Cursor *cursor, uint8_t *ptr, uintptr_t len);

int64_t read_next_entry(struct DirectoryEntries *entries, struct RawDirEntry *entry);

void write(struct Cursor *cursor, const uint8_t *ptr, uintptr_t len);
