import struct
import unittest
from smcextract import decrypt, extract


class SmcExtractTests(unittest.TestCase):
    def test_cipher_feedback_vector(self):
        self.assertEqual(decrypt(bytes.fromhex('00010203')), bytes.fromhex('42744b6c'))

    def test_extract_bounds(self):
        image = bytearray(0x1000)
        image[:2] = b'\xff\x4f'
        struct.pack_into('>II', image, 0x78, 0x300, 0x100)
        raw, plain, info = extract(image)
        self.assertEqual(len(raw), 0x300)
        self.assertEqual(plain[:4], b'BuNy')
        self.assertEqual(info['offset'], 0x100)
        struct.pack_into('>II', image, 0x78, 0x300, 0xf00)
        with self.assertRaises(ValueError):
            extract(image)

    def test_reject_short_or_wrong_magic(self):
        for data in (b'', bytes(0x1000)):
            with self.assertRaises(ValueError):
                extract(data)


if __name__ == '__main__':
    unittest.main()
