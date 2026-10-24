#pragma once
#include <intrin.h>
#include <string.h>
#define strcasecmp _stricmp
static __inline int __builtin_ffs(int x) { unsigned long i; return _BitScanForward(&i, (unsigned long)x) ? (int)i + 1 : 0; }
