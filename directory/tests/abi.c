/* Read-only SDK contract check. Compile with the supported macOS SDK:
 * clang -std=c11 -Wall -Werror -fsyntax-only -isysroot <SDK> directory/tests/abi.c
 * Runtime symbol/error/prefix checks are the separate Rust sys tests.
 */
#include <stddef.h>
#include <stdint.h>
#include <dirent.h>
#include <sys/mount.h>
_Static_assert(sizeof(size_t) == 8, "size_t");
_Static_assert(sizeof(off_t) == 8, "off_t");
_Static_assert(sizeof(struct dirent) == 1048, "dirent size");
_Static_assert(offsetof(struct dirent, d_ino) == 0, "inode");
_Static_assert(offsetof(struct dirent, d_seekoff) == 8, "seek");
_Static_assert(offsetof(struct dirent, d_reclen) == 16, "record length");
_Static_assert(offsetof(struct dirent, d_namlen) == 18, "name length");
_Static_assert(offsetof(struct dirent, d_type) == 20, "type");
_Static_assert(offsetof(struct dirent, d_name) == 21, "name");
_Static_assert(sizeof(struct statfs) == 2168, "statfs size");
_Static_assert(offsetof(struct statfs, f_flags) == 64, "mount flags");
_Static_assert(sizeof(((struct statfs *)0)->f_flags) == 4, "mount flag width");
_Static_assert(MNT_UNION == 0x20, "union flag");
int (*const checked_fstatfs)(int, struct statfs *) = fstatfs;

#include <sys/stat.h>
#include <fcntl.h>
_Static_assert(sizeof(struct stat) == 144, "stat size");
_Static_assert(_Alignof(struct stat) == 8, "stat alignment");
_Static_assert(offsetof(struct stat, st_mode) == 4, "stat mode offset");
_Static_assert(sizeof(((struct stat *)0)->st_mode) == 2, "stat mode width");
_Static_assert(AT_SYMLINK_NOFOLLOW == 0x0020, "nofollow entry flag");
int (*const checked_fstatat)(int, const char *, struct stat *, int) = fstatat;
