import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from independent_hls import sliding_playlist


class IndependentHlsTests(unittest.TestCase):
    def test_sliding_window_uses_unique_continuous_segments_and_interior_time(self):
        original = '#EXTM3U\n#EXT-X-TARGETDURATION:2\n' + ''.join(f'#EXTINF:2.0,\nsegment-{i:03d}.ts\n' for i in range(150)) + '#EXT-X-ENDLIST\n'
        first = sliding_playlist(original, 0)
        later = sliding_playlist(original, 80, True)
        self.assertIn('#EXT-X-MEDIA-SEQUENCE:24\n', first)
        self.assertIn('segment-047.ts', first)
        self.assertNotIn('EXT-X-START', first)
        self.assertNotIn('ENDLIST', first)
        self.assertIn('#EXT-X-MEDIA-SEQUENCE:64\n', later)
        self.assertIn('segment-087.ts', later)
        self.assertIn('EXT-X-START', later)
        expired = sliding_playlist(original, 1000)
        self.assertIn('segment-149.ts', expired)
        self.assertIn('ENDLIST', expired)
        self.assertNotIn('segment-000.ts', expired)


if __name__ == '__main__':
    unittest.main()
