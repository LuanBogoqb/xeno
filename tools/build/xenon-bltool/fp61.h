#pragma once
#include <intrin.h>
#include <stdint.h>
// local MSVC build shim: replaces the __int128 version
#define CAT_MUL128(r_hi, r_lo, x, y) { unsigned __int64 hi_; r_lo = _umul128((unsigned __int64)(x), (unsigned __int64)(y), &hi_); r_hi = hi_; }
