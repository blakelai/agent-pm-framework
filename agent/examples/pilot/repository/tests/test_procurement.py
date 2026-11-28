import unittest

from src.procurement import approval_route


class CapturedRoutingTests(unittest.TestCase):
    def test_at_captured_threshold(self):
        self.assertEqual(approval_route(50_000), ("department_manager",))

    def test_first_amount_above_captured_threshold(self):
        self.assertEqual(
            approval_route(50_001), ("department_manager", "finance_queue")
        )


if __name__ == "__main__":
    unittest.main()
