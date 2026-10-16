#!/usr/bin/env sh
# The local checks of a stage, each build form, errors only. No CI: this is the gate.
#   sh tools/check.sh          all checks
#   sh tools/check.sh fast     format and static clippy only (the pre-commit hook)
# The dynamic form compiles code that the static form does not (`maki-engine`
# feature `dynamic`: the editor program, "New script", the reload).
cd "$(dirname "$0")/.." || exit 1
failed=0

step() {
    name=$1
    shift
    out=$("$@" 2>&1)
    status=$?
    # A crash of rustc (STATUS_STACK_BUFFER_OVERRUN and others) is a fail, also when cargo ends with 0.
    errors=$(printf '%s\n' "$out" | rg -e '^(error|warning)|FAILED|panicked|^Diff in|Error:|^FAIL |STATUS_[A-Z_]+|exit code: 0xc' -A 5 | head -40)
    if [ $status -ne 0 ] || [ -n "$errors" ]; then
        echo "FAIL $name"
        printf '%s\n' "$errors"
        failed=1
    else
        echo "ok   $name"
    fi
}

step "format" cargo fmt --all -- --check
# The Python tools: a syntax error (a parse writes no cache files), and the
# self-test of the benchmark statistics.
tools() {
    python -B -c 'import ast, glob; [ast.parse(open(p, encoding="utf-8").read(), p) for p in glob.glob("tools/*.py")]' &&
        python -B tools/bench.py selftest &&
        python -B tools/comment_lint.py selftest
}
step "tools" tools
# One-line comments, in the lines that this branch adds (CODING_STANDARDS.md).
step "comments" python -B tools/comment_lint.py
step "clippy static" cargo clippy -q --workspace --all-targets
if [ "$1" != "fast" ]; then
    step "test static" cargo test -q
    step "build dynamic" cargo build -q --profile dynamic -p maki-editor-app -p maki-game-dll
    step "clippy dynamic" cargo clippy -q --profile dynamic -p maki-editor-app -p maki-game-dll -p maki-engine --features maki-engine/dynamic
    # A program of the profile `dynamic` needs the DLL of the standard library.
    PATH="$(cygpath "$(rustc --print sysroot)" 2>/dev/null || rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/lib:$PATH"
    export PATH
    step "test dynamic" cargo test -q --profile dynamic -p maki-editor-app -p maki-game-dll
    # The tests of reload.rs compile only with the feature `dynamic`.
    step "test reload" cargo test -q --profile dynamic -p maki-engine --features dynamic --lib
fi
exit $failed
