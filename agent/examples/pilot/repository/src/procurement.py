"""Approval routing in the synthetic repository revision."""

SECOND_APPROVAL_THRESHOLD_TWD = 50_000


def approval_route(total_twd: int) -> tuple[str, ...]:
    """Return the approval roles for a validated whole-TWD total."""
    if total_twd > SECOND_APPROVAL_THRESHOLD_TWD:
        return ("department_manager", "finance_queue")
    return ("department_manager",)
