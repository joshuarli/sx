from fixture_marks import source_runs_in_python


def test_only_a_leading_comment_line_marks_a_source_as_python_free() -> None:
    assert not source_runs_in_python("; python: no\n1")
    assert not source_runs_in_python("; task: t\n; allow: a.b\n; python: no\n(+ 1 2)")
    assert source_runs_in_python("; task: t\n(+ 1 2)\n; python: no")
    assert source_runs_in_python("; task: t\n1")
    assert source_runs_in_python("; python: nope\n1")
