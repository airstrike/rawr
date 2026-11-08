// mmap a JSON file read-only and, for each value at one of the given key
// paths (array levels skipped, as jsonspan prints them), count values,
// distinct values (by a 64-bit hash of their bytes), total and distinct bytes.
// Usage: jsondistinct FILE PATH [PATH ...]
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#define MAXD 128
#define MAXP 8

typedef struct {
    const char *path;
    uint64_t *set;
    uint64_t cap, used, count, bytes, distinct_bytes;
} Target;

static Target targets[MAXP];
static int ntargets;

static uint64_t hash(const unsigned char *p, size_t n) {
    uint64_t h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++) h = (h ^ p[i]) * 1099511628211ull;
    h ^= n * 0x9e3779b97f4a7c15ull;
    return h ? h : 1;
}

static void insert(Target *t, uint64_t h, size_t n) {
    if (t->used * 2 >= t->cap) {
        uint64_t ncap = t->cap ? t->cap * 2 : (1u << 20);
        uint64_t *nset = calloc(ncap, sizeof(uint64_t));
        for (uint64_t i = 0; i < t->cap; i++) {
            if (!t->set[i]) continue;
            uint64_t j = t->set[i] & (ncap - 1);
            while (nset[j]) j = (j + 1) & (ncap - 1);
            nset[j] = t->set[i];
        }
        free(t->set);
        t->set = nset;
        t->cap = ncap;
    }
    uint64_t j = h & (t->cap - 1);
    while (t->set[j]) {
        if (t->set[j] == h) return;
        j = (j + 1) & (t->cap - 1);
    }
    t->set[j] = h;
    t->used++;
    t->distinct_bytes += n;
}

int main(int argc, char **argv) {
    int fd = open(argv[1], O_RDONLY);
    struct stat st;
    fstat(fd, &st);
    const unsigned char *d = mmap(NULL, st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (d == MAP_FAILED) return 1;
    for (int i = 2; i < argc && ntargets < MAXP; i++) targets[ntargets++].path = argv[i];
    char type[MAXD];
    size_t keystart[MAXD];
    int haskey[MAXD], pathlen[MAXD];
    char path[8192] = "";
    char lastkey[512] = "";
    int depth = 0, instr = 0, esc = 0, readingkey = 0, expectkey = 0, klen = 0;
    char key[512];
    for (size_t pos = 0; pos < (size_t)st.st_size; pos++) {
        unsigned char c = d[pos];
        if (instr) {
            if (esc) { esc = 0; if (readingkey && klen < 511) key[klen++] = c; continue; }
            if (c == '\\') { esc = 1; continue; }
            if (c == '"') { instr = 0; if (readingkey) { key[klen] = 0; strcpy(lastkey, key); readingkey = 0; } continue; }
            if (readingkey && klen < 511) key[klen++] = c;
            continue;
        }
        int close_key = 0;
        switch (c) {
        case '"':
            instr = 1;
            if (depth > 0 && type[depth - 1] == '{' && expectkey) { readingkey = 1; klen = 0; expectkey = 0; }
            break;
        case ':':
            if (depth > 0 && type[depth - 1] == '{') {
                int pl = pathlen[depth - 1];
                path[pl] = 0;
                char *p = path + pl;
                if (pl) *p++ = '.';
                strcpy(p, lastkey);
                keystart[depth - 1] = pos + 1;
                haskey[depth - 1] = 1;
            }
            break;
        case ',':
            if (depth > 0 && type[depth - 1] == '{') { close_key = 1; expectkey = 1; }
            break;
        case '{':
        case '[':
            type[depth] = c;
            haskey[depth] = 0;
            pathlen[depth] = (int)strlen(path);
            if (c == '{') expectkey = 1;
            depth++;
            break;
        case '}':
        case ']':
            depth--;
            if (type[depth] == '{' && haskey[depth]) {
                // close the object's last key value at this position
                for (int t = 0; t < ntargets; t++) {
                    if (strcmp(path, targets[t].path) == 0) {
                        size_t n = pos - keystart[depth];
                        targets[t].count++;
                        targets[t].bytes += n;
                        insert(&targets[t], hash(d + keystart[depth], n), n);
                    }
                }
            }
            path[pathlen[depth]] = 0;
            break;
        }
        if (close_key && haskey[depth - 1]) {
            for (int t = 0; t < ntargets; t++) {
                if (strcmp(path, targets[t].path) == 0) {
                    size_t n = pos - keystart[depth - 1];
                    targets[t].count++;
                    targets[t].bytes += n;
                    insert(&targets[t], hash(d + keystart[depth - 1], n), n);
                }
            }
            haskey[depth - 1] = 0;
            path[pathlen[depth - 1]] = 0;
        }
    }
    for (int t = 0; t < ntargets; t++) {
        Target *x = &targets[t];
        printf("%-50s values=%llu distinct=%llu bytes=%.1fMiB distinct_bytes=%.1fMiB\n", x->path,
               (unsigned long long)x->count, (unsigned long long)x->used, x->bytes / 1048576.0, x->distinct_bytes / 1048576.0);
    }
    return 0;
}
