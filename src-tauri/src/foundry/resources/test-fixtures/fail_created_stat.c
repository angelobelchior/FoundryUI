#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>

static int fail_created_stat(int descriptor, struct stat *metadata) {
    int result = fstat(descriptor, metadata);
    char path[4096];
    if (result == 0 && S_ISREG(metadata->st_mode) && fcntl(descriptor, F_GETPATH, path) == 0) {
        const char *name = strrchr(path, '/');
        if (name && strcmp(name + 1, "post-create-validation") == 0) {
            errno = EIO;
            return -1;
        }
    }
    return result;
}

__attribute__((used)) static struct {
    const void *replacement;
    const void *original;
} interposition __attribute__((section("__DATA,__interpose"))) = {
    (const void *)&fail_created_stat, (const void *)&fstat
};
