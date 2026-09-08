import importlib.util
from pathlib import Path
import unittest

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/starlink-prefixes.py"
spec = importlib.util.spec_from_file_location("starlink_prefixes", source)
prefixes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prefixes)


class PrefixTests(unittest.TestCase):
    def test_rejects_empty_private_and_overbroad_lists(self):
        for values in [[], ["0.0.0.0/0", "2001:4860::/32"],
                       ["10.0.0.0/8", "2001:4860::/32"],
                       ["8.8.8.0/24"], ["8.8.8.0/24", "::/0"],
                       ["8.8.8.0/24; accept", "2001:4860::/32"]]:
            with self.subTest(values=values), self.assertRaises(ValueError):
                prefixes.validate_prefixes(values)

    def test_collapses_overlapping_announcements(self):
        result = prefixes.validate_prefixes([
            "8.8.8.0/24", "8.8.8.0/25", "2001:4860::/32",
        ])
        self.assertEqual(result, ["8.8.8.0/24", "2001:4860::/32"])

    def test_empty_cache_denies_both_families_without_flushing_other_tables(self):
        rules = prefixes.ruleset([])
        self.assertNotIn("flush ruleset", rules)
        self.assertIn("destroy table inet victron_ingress", rules)
        self.assertIn("ip saddr != @starlink4 drop", rules)
        self.assertIn("ip6 saddr != @starlink6 drop", rules)
        self.assertNotIn("elements", rules)


if __name__ == "__main__":
    unittest.main()
