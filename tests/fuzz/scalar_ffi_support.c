#include <stdint.h>

int64_t fuzz_add_i64(int64_t left, int64_t right) {
    return left + right;
}

float fuzz_scale_f32(float left, float right) {
    return left * right;
}

double fuzz_scale_f64(double left, double right) {
    return left * right;
}

void fuzz_sink_i64(int64_t value) {
    (void)value;
}