#!/usr/bin/env python3
"""Run the deterministic adversarial Vaca corpus against a release compiler."""

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


CORPUS = Path(__file__).resolve().parent
ROOT = CORPUS.parents[1]
DEFAULT_COMPILER = ROOT / "target" / "release" / "vacac"
TIMEOUT_SECONDS = 60


def run(command, *, timeout=TIMEOUT_SECONDS, **kwargs):
    return subprocess.run(
        [str(part) for part in command],
        capture_output=True,
        text=True,
        timeout=timeout,
        **kwargs,
    )


def check_result(label, result, *, code=0, stderr_contains=None, stdout=None):
    errors = []
    if result.returncode != code:
        errors.append(f"exit {result.returncode}, expected {code}")
    if stderr_contains and stderr_contains not in result.stderr:
        errors.append(f"stderr does not contain {stderr_contains!r}")
    if stdout is not None and result.stdout != stdout:
        errors.append(f"stdout {result.stdout!r}, expected {stdout!r}")
    if "panicked at" in result.stderr or "internal compiler error" in result.stderr.lower():
        errors.append("compiler reported an internal panic/error")
    if errors:
        print(f"[FAIL] {label}: {'; '.join(errors)}")
        if result.stdout:
            print(f"  stdout: {result.stdout!r}")
        if result.stderr:
            print(f"  stderr: {result.stderr}")
        return False
    print(f"[PASS] {label}")
    return True


def diagnostic_case(compiler, label, filename, expected_message):
    return check_result(
        label,
        run([compiler, CORPUS / filename, "--check", "--json"]),
        code=1,
        stderr_contains=expected_message,
    )


def parser_json_case(compiler):
    label = "parser recovery emits multiple valid JSON diagnostics"
    result = run([compiler, CORPUS / "parser_recovery.vca", "--check", "--json"])
    valid = result.returncode == 1 and not result.stdout
    try:
        diagnostics = [json.loads(line) for line in result.stderr.splitlines()]
    except json.JSONDecodeError as error:
        diagnostics = []
        valid = False
        parse_error = str(error)
    else:
        parse_error = ""
    required = {"severity", "file", "line", "column", "end_column", "message"}
    valid = valid and len(diagnostics) == 2
    valid = valid and all(required <= diagnostic.keys() for diagnostic in diagnostics)
    valid = valid and all(diagnostic["severity"] == "error" for diagnostic in diagnostics)
    valid = valid and {diagnostic["line"] for diagnostic in diagnostics} == {3, 7}
    valid = valid and all(diagnostic["file"] for diagnostic in diagnostics)
    if "panicked at" in result.stderr:
        valid = False
    if valid:
        print(f"[PASS] {label}")
        return True
    print(f"[FAIL] {label}: got {result.returncode}, {len(diagnostics)} parsed diagnostics {parse_error}")
    print(f"  stdout: {result.stdout!r}")
    print(f"  stderr: {result.stderr}")
    return False


def malformed_match_json_case(compiler):
    label = "malformed match arm reports valid JSON and recovery terminates"
    result = run([compiler, CORPUS / "parser_match_recovery.vca", "--check", "--json"])
    try:
        diagnostics = [json.loads(line) for line in result.stderr.splitlines()]
    except json.JSONDecodeError as error:
        diagnostics = []
        parse_error = str(error)
    else:
        parse_error = ""
    valid = result.returncode == 1 and not result.stdout and len(diagnostics) == 1
    valid = valid and all(
        {"severity", "file", "line", "column", "end_column", "message"} <= diagnostic.keys()
        for diagnostic in diagnostics
    )
    valid = valid and "expected `;` or `}`" in diagnostics[0]["message"] if diagnostics else False
    valid = valid and "panicked at" not in result.stderr
    if valid:
        print(f"[PASS] {label}")
        return True
    print(f"[FAIL] {label}: got {result.returncode}, {len(diagnostics)} parsed diagnostics {parse_error}")
    print(f"  stdout: {result.stdout!r}")
    print(f"  stderr: {result.stderr}")
    return False


def make_deep_modules(temporary_directory):
    depth = 48
    project = Path(temporary_directory) / "deep_modules"
    shutil.rmtree(project, ignore_errors=True)
    project.mkdir(parents=True)
    chain = [f"level_{index}" for index in range(depth)]
    qualified = "::".join(chain)
    (project / "main.vca").write_text(
        f"mod {chain[0]};\nfn main() -> i64 {{ {qualified}::value() - 42 }}\n",
        encoding="utf-8",
    )
    for index, module in enumerate(chain):
        directory = project.joinpath(*chain[:index])
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / f"{module}.vca"
        if index + 1 == depth:
            source = "pub fn value() -> i64 { 42 }\n"
        else:
            child = chain[index + 1]
            source = f"mod {child};\npub fn value() -> i64 {{ {child}::value() }}\n"
        path.write_text(source, encoding="utf-8")
    return project / "main.vca"


def main():
    compiler = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else DEFAULT_COMPILER
    if not compiler.is_file():
        print(f"compiler binary not found: {compiler}", file=sys.stderr)
        return 2

    failures = 0

    def record(ok):
        nonlocal failures
        failures += not ok

    record(check_result(
        "shared-reference reborrow and generic lifetime return check",
        run([compiler, CORPUS / "shared_reborrow.vca", "--check", "--quiet"]),
    ))
    record(diagnostic_case(
        compiler, "mutable-reference reborrow is rejected", "mutable_reborrow_rejected.vca",
        "mutable reference reborrowing is not supported",
    ))
    record(diagnostic_case(
        compiler, "mutable loan blocks overlapping write", "mutable_loan_conflict.vca",
        "cannot access `value` while it is mutably borrowed",
    ))
    record(diagnostic_case(
        compiler, "overlapping mutable call arguments are rejected", "mutable_alias_call.vca",
        "overlapping mutable references cannot be passed to the same call",
    ))
    record(diagnostic_case(
        compiler, "wrong named lifetime origin is rejected", "lifetime_wrong_origin.vca",
        "returned reference has lifetime `'b`, expected `'a`",
    ))
    record(diagnostic_case(
        compiler, "local reference cannot escape its function", "lifetime_local_escape.vca",
        "cannot return a reference that does not originate from a parameter",
    ))
    record(diagnostic_case(
        compiler, "undeclared lifetime name is rejected", "lifetime_undeclared.vca",
        "undeclared lifetime `'b`",
    ))
    record(check_result(
        "distinct constant array places permit a mutable loan",
        run([compiler, CORPUS / "mutable_array_disjoint.vca", "--run"]),
    ))
    record(diagnostic_case(
        compiler, "unknown array index conservatively overlaps a mutable loan",
        "mutable_array_dynamic_conflict.vca",
        "cannot modify `values` while it is borrowed",
    ))
    record(diagnostic_case(
        compiler, "generic expansion hits the 512-instance diagnostic", "generic_specialization_limit.vca",
        "generic specialization exceeded the limit of 512",
    ))
    record(parser_json_case(compiler))
    record(malformed_match_json_case(compiler))

    record(check_result(
        "i64::MIN literal is valid",
        run([compiler, CORPUS / "i64_min_valid.vca", "--run"]),
        stdout="-9223372036854775808\n",
    ))
    record(check_result(
        "negating i64::MIN traps with status 1",
        run([compiler, CORPUS / "i64_min_overflow.vca", "--run"]),
        code=1,
    ))
    record(check_result(
        "IEEE infinities, NaN, f32, and signed zero",
        run([compiler, CORPUS / "float_ieee_edges.vca", "--run"]),
        stdout="inf\n-inf\nnan\n-inf\n-0\n",
    ))
    record(check_result(
        "returned closure retains its aggregate capture",
        run([compiler, CORPUS / "returned_closure_capture.vca", "--run"]),
    ))

    with tempfile.TemporaryDirectory(prefix="vacac-fuzz-modules-") as temporary:
        module_source = make_deep_modules(Path(temporary))
        record(check_result(
            "48-level nested module load, resolution, and execution",
            run([compiler, module_source, "--run"]),
        ))

    with tempfile.TemporaryDirectory(prefix="vacac-fuzz-ffi-") as temporary:
        object_file = Path(temporary) / "support.o"
        native = run(["cc", "-c", CORPUS / "scalar_ffi_support.c", "-o", object_file])
        if not check_result("compile scalar C FFI support object", native):
            failures += 1
        else:
            record(check_result(
                "scalar C FFI calls i64/f32/f64/unit functions",
                run([
                    compiler, CORPUS / "scalar_ffi.vca", "--run",
                    "--link-object", object_file,
                ]),
            ))

    with tempfile.TemporaryDirectory(prefix="vacac-fuzz-ir-") as temporary:
        ir_file = Path(temporary) / "closure.ll"
        result = run([
            compiler, CORPUS / "returned_closure_capture.vca",
            "--emit-llvm", ir_file, "--quiet",
        ])
        ok = result.returncode == 0 and ir_file.is_file() and ir_file.stat().st_size > 0
        if ok:
            ok = "define" in ir_file.read_text(encoding="utf-8")
        record(check_result("emit verified LLVM for closure capture", result) if ok else False)
        if not ok and result.stderr:
            print(f"  stderr: {result.stderr}")

    print(f"\nCorpus result: {failures} failure(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.TimeoutExpired as error:
        print(f"[FAIL] timeout after {error.timeout}s: {error.cmd}", file=sys.stderr)
        raise SystemExit(1)