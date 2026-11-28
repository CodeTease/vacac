#include <stddef.h>
#include <stdint.h>

const char *fuzz_label(void) {
    return "fuzz";
}

int64_t fuzz_fill(const char *label, int64_t *out) {
    if (label == NULL || out == NULL || label[0] != 'f') {
        return 0;
    }
    *out = 42;
    return 1;
}

int64_t *fuzz_null(void) {
    return 0;
}
