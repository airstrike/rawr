/* Read-only glibc and kernel header contract check for 64-bit Linux:
 * cc -std=gnu11 -Wall -Werror -fsyntax-only directory/tests/abi-linux.c
 * Runtime syscall/error/record checks are the separate Rust sys tests.
 */
#define _GNU_SOURCE
#include <stddef.h>
#include <stdint.h>
#include <dirent.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>
_Static_assert(sizeof(long) == 8, "long");
_Static_assert(sizeof(ino_t) == 8, "ino_t");
_Static_assert(sizeof(off_t) == 8, "off_t");
/* glibc's 64-bit dirent is the kernel's linux_dirent64. */
_Static_assert(offsetof(struct dirent, d_ino) == 0, "inode");
_Static_assert(offsetof(struct dirent, d_off) == 8, "offset");
_Static_assert(offsetof(struct dirent, d_reclen) == 16, "record length");
_Static_assert(sizeof(((struct dirent *)0)->d_reclen) == 2, "record length width");
_Static_assert(offsetof(struct dirent, d_type) == 18, "type");
_Static_assert(offsetof(struct dirent, d_name) == 19, "name");
_Static_assert(sizeof(((struct dirent *)0)->d_name) == 256, "NAME_MAX + 1");
_Static_assert(SYS_getdents64 > 0, "getdents64 number");
long (*const checked_syscall)(long, ...) = syscall;

_Static_assert(_Alignof(struct stat) == 8, "stat alignment");
_Static_assert(sizeof(((struct stat *)0)->st_mode) == 4, "stat mode width");
#if defined(__aarch64__)
_Static_assert(sizeof(struct stat) == 128, "stat size");
_Static_assert(offsetof(struct stat, st_mode) == 16, "stat mode offset");
#elif defined(__x86_64__)
_Static_assert(sizeof(struct stat) == 144, "stat size");
_Static_assert(offsetof(struct stat, st_mode) == 24, "stat mode offset");
#endif
_Static_assert(AT_SYMLINK_NOFOLLOW == 0x100, "nofollow entry flag");
int (*const checked_fstatat)(int, const char *, struct stat *, int) = fstatat;
