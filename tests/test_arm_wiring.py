import json
from pathlib import Path

import definition
import effects
import gate
import real
import repairs
import session
from arms import Files
from repairs import Tried

LEDGER = "ledger-charges"


def wired(arm: session.SessionArm, tmp_path: Path) -> session.Wiring:
    workflow = definition.load_effects(LEDGER)
    with workflow.sandbox() as sandbox:
        files = Files(workflow, sandbox, tmp_path)
        return session.wiring(arm, workflow, sandbox, files, tmp_path)


def test_the_ts_arm_is_the_sx_arm_with_the_language_flag_and_nothing_else(tmp_path: Path) -> None:
    sx_dir, ts_dir = tmp_path / "sx", tmp_path / "ts"
    sx_dir.mkdir()
    ts_dir.mkdir()
    sx, ts = wired("sx", sx_dir), wired("ts", ts_dir)
    assert ts.server[-2:] == ["--language", "ts"]
    assert "--language" not in sx.server
    assert [a.replace(str(ts_dir), "D") for a in ts.server[:-2]] == [a.replace(str(sx_dir), "D") for a in sx.server]
    assert (ts.tools, ts.extensions, ts.settings, ts.append_system_prompt) == (
        sx.tools, sx.extensions, sx.settings, sx.append_system_prompt,
    )
    assert ts.tools == "mcp__sx_code__*"
    assert ts.append_system_prompt is None


def test_the_js_arm_lists_every_served_tool_in_the_codemode_description(tmp_path: Path) -> None:
    js = wired("js", tmp_path)
    assert js.settings["codemode"] == {"mode": "only", "inlineBudget": session.CODEMODE_INLINE_BUDGET_TOKENS}
    assert js.tools == f"codemode,mcp__{effects.JS_SERVER}__*"
    project = tmp_path / "project"
    session.write_project(js, project, tmp_path / "proxy.jsonl")
    servers = json.loads((project / ".pi" / "mcp.json").read_text())["mcpServers"]
    assert servers[effects.JS_SERVER]["exposure"] == "direct"


def imported(tmp_path: Path) -> Path:
    target = tmp_path / "imported.json"
    target.write_text(json.dumps({"capabilities": [{"id": "repomod.status"}]}))
    return target


def configured(arm: session.SessionArm, listing: real.JsListing, tmp_path: Path) -> tuple[list[str], dict, dict]:
    directory = tmp_path / f"{arm}-{listing}"
    directory.mkdir()
    scratch = tmp_path / "scratch"
    scratch.mkdir(exist_ok=True)
    flags = real.configure(arm, directory, scratch, imported(tmp_path), listing)
    agent = directory / "agent"
    servers = json.loads((agent / "mcp.json").read_text())["mcpServers"]
    return flags, servers, json.loads((agent / "settings.json").read_text())


def test_the_real_js_arm_lists_tools_directly_or_leaves_them_to_search(tmp_path: Path) -> None:
    flags, servers, settings = configured("js", "direct", tmp_path)
    assert {s["exposure"] for s in servers.values()} == {"direct"}
    assert settings["codemode"]["mode"] == "only"
    assert flags[-1] == "read,grep,find,ls,codemode,mcp__repomod__*,mcp__files__*"
    flags, servers, settings = configured("js", "discovered", tmp_path)
    assert {s["exposure"] for s in servers.values()} == {"codemode"}
    assert "codemode" not in settings
    assert flags[-1] == "read,grep,find,ls,codemode"


def test_the_real_ts_arm_differs_from_the_sx_arm_by_the_language_flag(tmp_path: Path) -> None:
    sx_flags, sx_servers, sx_settings = configured("sx", "direct", tmp_path)
    ts_flags, ts_servers, ts_settings = configured("ts", "direct", tmp_path)
    assert (ts_flags, ts_settings) == (sx_flags, sx_settings)
    sx_args, ts_args = sx_servers["sx-code"]["args"], ts_servers["sx-code"]["args"]
    split = ts_args.index("--language")
    assert ts_args[split : split + 2] == ["--language", "ts"]
    assert [a.replace("ts-direct", "sx-direct") for a in ts_args[:split] + ts_args[split + 2 :]] == sx_args


def test_a_run_that_trapped_carries_its_trap_kind_as_a_code() -> None:
    structured = {"outcome": "failed", "failure": {"kind": "DivideByZero", "message": "x"}, "trace": []}
    assert effects.sx_status("run", structured) == ("failed", ("trap:DivideByZero",), ())


def test_repairs_credit_every_reason_of_a_stopped_program_followed_by_another_attempt() -> None:
    first = [
        Tried("rejected", ("ts-mutation", "ts-mutation", "type-mismatch")),
        Tried("refused", ("out-of-scope",)),
        Tried("completed", ()),
    ]
    second = [Tried("checked", ()), Tried("failed", ("trap:Timeout",)), Tried("failed", ())]
    counts = {c.reason: (c.programs, c.repairs) for c in repairs.tally([first, second])}
    assert counts == {
        "ts-mutation": (1, 1),
        "type-mismatch": (1, 1),
        "out-of-scope": (1, 1),
        "trap:Timeout": (1, 1),
        repairs.UNCODED: (1, 0),
    }
    assert repairs.tally([first, second])[-1].reason == repairs.UNCODED


def test_the_gate_compares_ts_with_js_per_model_and_pooled() -> None:
    scored = [
        *[gate.Scored("m", "js", True, 0.01) for _ in range(20)],
        *[gate.Scored("m", "ts", i != 0, 0.012) for i in range(20)],
        *[gate.Scored("n", "js", True, 0.01) for _ in range(10)],
        *[gate.Scored("n", "ts", i > 1, 0.02) for i in range(10)],
    ]
    found = gate.cells(scored)
    gap, ratio, passed = gate.verdict(found["m", "ts"], found["m", "js"])
    assert (round(gap, 1), round(ratio or 0, 2), passed) == (-5.0, 1.2, True)
    gap, ratio, passed = gate.verdict(found["n", "ts"], found["n", "js"])
    assert (round(gap, 1), round(ratio or 0, 2), passed) == (-20.0, 2.0, False)
    pooled = gate.verdict(found[gate.POOLED, "ts"], found[gate.POOLED, "js"])
    assert (round(pooled[0], 1), pooled[2]) == (-10.0, False)
    shown = gate.lines(scored, ("js", "ts"))
    assert any(line.startswith("| all models | -10.0") and line.endswith("fail |") for line in shown)
