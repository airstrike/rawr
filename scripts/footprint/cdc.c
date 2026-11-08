// Cut files into content-defined parts exactly as importer/src/api/parts.rs
// does (gear hash, 16..256 KiB, boundary mask 2^16-1), hash each part, and
// report how many parts and bytes the files share: the bytes a parts store
// would keep for all of them together. Read-only (mmap).
// Usage: cdc FILE [FILE ...]
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#define MIN_PART (16 * 1024)
#define MAX_PART (256 * 1024)
#define BOUNDARY ((1ull << 16) - 1)

static uint64_t GEAR[256];

static void gear_init(void) {
    uint64_t state = 0x5241575241504931ull;
    for (int i = 0; i < 256; i++) {
        state += 0x9e3779b97f4a7c15ull;
        uint64_t v = state;
        v = (v ^ (v >> 30)) * 0xbf58476d1ce4e5b9ull;
        v = (v ^ (v >> 27)) * 0x94d049bb133111ebull;
        GEAR[i] = v ^ (v >> 31);
    }
}

static size_t boundary(const unsigned char *b, size_t n) {
    if (n <= MIN_PART) return n;
    size_t end = n < MAX_PART ? n : MAX_PART;
    uint64_t h = 0;
    for (size_t i = MIN_PART; i < end; i++) {
        h = (h << 1) + GEAR[b[i]];
        if ((h & BOUNDARY) == 0) return i + 1;
    }
    return end;
}

static uint64_t fnv(const unsigned char *p, size_t n) {
    uint64_t h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++) h = (h ^ p[i]) * 1099511628211ull;
    h ^= n * 0x9e3779b97f4a7c15ull;
    return h ? h : 1;
}

static uint64_t *set;
static uint64_t cap = 1 << 22, used;

static int insert(uint64_t h) {
    uint64_t j = h & (cap - 1);
    while (set[j]) {
        if (set[j] == h) return 0;
        j = (j + 1) & (cap - 1);
    }
    set[j] = h;
    used++;
    return 1;
}

int main(int argc, char **argv) {
    gear_init();
    set = calloc(cap, sizeof(uint64_t));
    unsigned long long union_bytes = 0, logical = 0;
    for (int a = 1; a < argc; a++) {
        int fd = open(argv[a], O_RDONLY);
        struct stat st;
        fstat(fd, &st);
        const unsigned char *d = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
        size_t off = 0;
        unsigned long long parts = 0, newparts = 0, newbytes = 0;
        while (off < (size_t)st.st_size) {
            size_t n = boundary(d + off, st.st_size - off);
            parts++;
            if (insert(fnv(d + off, n))) { newparts++; newbytes += n; }
            off += n;
        }
        logical += st.st_size;
        union_bytes += newbytes;
        printf("%s: %.1f MiB, %llu parts, %llu new parts, %.1f MiB new\n", argv[a], st.st_size / 1048576.0, parts, newparts, newbytes / 1048576.0);
        munmap((void *)d, st.st_size);
        close(fd);
    }
    printf("logical %.1f MiB, kept as parts %.1f MiB (%.1f%%)\n", logical / 1048576.0, union_bytes / 1048576.0, 100.0 * union_bytes / logical);
    return 0;
}
