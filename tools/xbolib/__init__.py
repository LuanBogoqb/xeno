"""Shared Xbox 360 formats and crypto used by the command-line tools in tools/.

Modules
-------
binary   big/little-endian field readers and alignment helpers
crypto   AES (ECB/CBC), RC4 and the HMAC-SHA1 key derivation
lzx      the Xbox 360 LZX variant (32 KiB frames, no reset, E8 translation)
xex      XEX2 header parsing, decryption + decompression, imports, strings
boot     flash boot chain (CB_A..CG) decryption and kernel reconstruction
"""
