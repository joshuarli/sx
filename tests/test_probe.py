from probe import judge

RETRY_TASK = {"expected": "done", "kinds": ["retry"]}


def test_an_immediate_retry_scores_the_same_with_or_without_a_zero_wait() -> None:
    assert judge(RETRY_TASK, "done", [0, 0], [], None) == ("correct", "")
    assert judge(RETRY_TASK, "done", [], [0], None) == ("correct", "")


def test_a_real_wait_still_has_to_match() -> None:
    assert judge(RETRY_TASK, "done", [0, 100], [100, 100], None)[0] == "wrong"
