import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest
from conftest import REPOMOD
from fleet import NOW, Fleet, build_fleet
from mcp_client import McpClient
from sx_repomod.capabilities import Capabilities, CapabilityError, Environment
from sx_repomod.declarations import CAPABILITIES, host_declarations

SERVE_FLEET = Path(__file__).with_name("serve_fleet.py")


@pytest.fixture
def fleet(tmp_path: Path, git_environment: None, monkeypatch: pytest.MonkeyPatch) -> Fleet:
    monkeypatch.setenv("PYTHONPATH", os.pathsep.join([str(REPOMOD / "src"), os.environ.get("PYTHONPATH", "")]))
    return build_fleet(tmp_path, monkeypatch)


def server_command(tmp_path: Path) -> list[str]:
    return [sys.executable, str(SERVE_FLEET), str(tmp_path), str(NOW)]


def kernel() -> str:
    return os.environ["SX_KERNEL"]


def sx(*arguments: str) -> str:
    done = subprocess.run(
        [kernel(), *arguments], capture_output=True, text=True, encoding="utf-8", check=False
    )
    assert done.returncode == 0, done.stderr
    return done.stdout


def exported(tmp_path: Path) -> dict[str, Any]:
    declarations = tmp_path / "host-declarations.json"
    declarations.write_text(json.dumps(host_declarations()))
    listing: dict[str, Any] = json.loads(sx("mcp", "export", "--declarations", str(declarations)))
    return listing


def in_process(fleet: Fleet) -> Capabilities:
    return Capabilities(Environment(lambda: fleet.catalog, lambda: float(NOW)))


def test_tools_list_is_what_sx_mcp_export_lists(tmp_path: Path, fleet: Fleet) -> None:
    with McpClient(server_command(tmp_path)) as client:
        initialized = client.initialize()
        listed = client.request("tools/list")
        assert client.close() == 0
    assert initialized["result"]["capabilities"] == {"tools": {"listChanged": False}}
    assert listed["result"]["tools"] == exported(tmp_path)["tools"]
    assert [tool["name"] for tool in listed["result"]["tools"]] == sorted(row[0] for row in CAPABILITIES)


@pytest.mark.parametrize(
    ("name", "arguments"),
    [
        ("catalog.select", {"repos": [], "tags": ["fork"]}),
        ("catalog.owner", {"owner": "acme"}),
        ("clock.now", {}),
        ("git.status", {"repo": "acme/dirty"}),
        ("git.status", {"repo": "acme/wrong-origin"}),
        ("git.oldest-unpushed", {"repo": "acme/ahead"}),
        ("repo.todo-items", {"repo": "acme/clean"}),
        ("github.cached-ci", {"repo": "acme/clean", "refresh": False, "now": NOW * 1000}),
        ("github.cached-upstream", {"repo": "acme/forked", "now": NOW * 1000}),
        ("policy.lint", {"repos": ["acme/clean"]}),
    ],
)
def test_a_call_answers_like_the_capability_in_process(
    name: str, arguments: dict[str, Any], tmp_path: Path, fleet: Fleet
) -> None:
    expected = in_process(fleet).call(name, arguments)
    shape = exported(tmp_path)["shapes"][name]
    with McpClient(server_command(tmp_path)) as client:
        client.initialize()
        result = client.tool(name, arguments)
    assert result["isError"] is False
    if shape == "text":
        assert result["content"] == [{"type": "text", "text": expected}]
        return
    content = expected if shape == "structured" else {"result": expected}
    assert result["structuredContent"] == content
    assert json.loads(result["content"][0]["text"]) == content


def test_a_capability_error_is_an_error_result_that_carries_its_kind(tmp_path: Path, fleet: Fleet) -> None:
    arguments = {"repo": "acme/nope"}
    with pytest.raises(CapabilityError) as raised:
        in_process(fleet).call("git.status", arguments)
    with McpClient(server_command(tmp_path)) as client:
        client.initialize()
        result = client.tool("git.status", arguments)
        followed = client.tool("clock.now", {})
    assert result["isError"] is True
    assert result["structuredContent"] == {"kind": raised.value.kind, "message": raised.value.message}
    assert result["content"] == [{"type": "text", "text": f"{raised.value.kind}: {raised.value.message}"}]
    assert raised.value.kind == "NotFound"
    assert followed["isError"] is False


def test_arguments_that_do_not_fit_the_input_type_are_invalid_and_unknown_tools_are_rejected(
    tmp_path: Path, fleet: Fleet
) -> None:
    with McpClient(server_command(tmp_path)) as client:
        client.initialize()
        missing = client.tool("catalog.select", {"repos": []})
        mistyped = client.tool("git.status", {"repo": 3})
        unknown = client.request("tools/call", {"name": "git.fetch", "arguments": {}})
    assert missing["isError"] is True
    assert missing["structuredContent"]["kind"] == "Invalid"
    assert mistyped["structuredContent"]["kind"] == "Invalid"
    assert unknown["error"]["code"] == -32602


def test_importing_the_server_reads_back_every_capability_but_those_with_payload_enum_outputs(
    tmp_path: Path, fleet: Fleet
) -> None:
    imported = json.loads(sx("mcp", "import", "--name", "repomod", "--", *server_command(tmp_path)))
    by_id = {capability["id"]: capability for capability in imported["capabilities"]}
    payload_enum_outputs = {"git.status", "github.cached-ci"}
    assert set(by_id) == {
        f"mcp-repomod.{row[0].replace('.', '-')}" for row in CAPABILITIES if row[0] not in payload_enum_outputs
    }
    for capability_id, input_type, _, _ in CAPABILITIES:
        if capability_id in payload_enum_outputs:
            continue
        capability = by_id[f"mcp-repomod.{capability_id.replace('.', '-')}"]
        assert capability["effect"] == "read"
        assert capability["input"] == input_type
    assert by_id["mcp-repomod.catalog-select"]["output"] == (
        "Record {result: List<Record {id: String, owner: String}>}"
    )
    assert by_id["mcp-repomod.catalog-owner"]["output"] == "String"


def test_sx_mcp_code_runs_a_program_over_the_server(tmp_path: Path, fleet: Fleet) -> None:
    imported = sx("mcp", "import", "--name", "repomod", "--", *server_command(tmp_path))
    (tmp_path / "declarations.json").write_text(imported)
    (tmp_path / "grants.json").write_text(json.dumps({"mcp-repomod.catalog-select": None}))
    (tmp_path / "policy.json").write_text(
        json.dumps({"capabilities": ["mcp-repomod.catalog-select"], "computed_inputs": []})
    )
    program = "(count (get (call mcp-repomod.catalog-select {:repos [] :tags [\"fork\"]}) :result))"
    command = [
        kernel(),
        "mcp",
        "code",
        "--declarations",
        str(tmp_path / "declarations.json"),
        "--grants",
        str(tmp_path / "grants.json"),
        "--policy",
        str(tmp_path / "policy.json"),
        "--",
        *server_command(tmp_path),
    ]
    with McpClient(command) as client:
        client.initialize()
        result = client.tool("run", {"program": program})
        assert client.close() == 0
    assert result["isError"] is False, result
    expected = in_process(fleet).call("catalog.select", {"repos": [], "tags": ["fork"]})
    assert result["structuredContent"]["result"] == len(expected) == 2
    trace = result["structuredContent"]["trace"]
    assert [call["capability"] for call in trace if "capability" in call] == ["mcp-repomod.catalog-select"]
